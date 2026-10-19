use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use stackly_domain::{Os, SecretRef};
use stackly_platform::{
    Interaction, SECRET_SERVICE, SecretBackend, SecretPresence, SecretStore, SecretStoreError,
    SecretValue, StoreState, WriteMode,
};

/// One fallible [`SecretStore`] method, for the call log of
/// [`FakeSecretStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretOp {
    /// [`SecretStore::probe`].
    Probe,
    /// [`SecretStore::read`].
    Read,
    /// [`SecretStore::write`].
    Write,
    /// [`SecretStore::delete`].
    Delete,
}

/// One call that reached a [`FakeSecretStore`], in the order it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretCall {
    /// The method.
    pub op: SecretOp,
    /// The reference it was called with.
    pub reference: SecretRef,
    /// The interaction it was called with; a probe has none.
    pub interaction: Option<Interaction>,
}

/// A secret store in memory that the test can lock, deny, switch off,
/// hold, and fail, and that counts every call (spec 0011).
///
/// It behaves like the Keychain adapter: a create on an existing item
/// fails `AlreadyExists`, a replace or delete on a missing one fails
/// `NotFound`, and [`state`](SecretStore::state) is the cache of what the
/// last call saw (`Unknown` until the first one). Clones share the store.
///
/// ```
/// use stackly_domain::SecretRef;
/// use stackly_platform::{Interaction, SecretStore, SecretStoreError, StoreState};
/// use stackly_testkit::{FakeSecretStore, SecretOp};
///
/// let store = FakeSecretStore::new();
/// let reference = SecretRef::parse("local:erp/admin")?;
/// store.insert(&reference, "not-a-real-secret");
/// store.lock();
///
/// let err = store.read(&reference, Interaction::Never).unwrap_err();
/// assert_eq!(err, SecretStoreError::Locked);
/// assert_eq!(store.state(), StoreState::Locked);
/// assert_eq!(store.count(SecretOp::Read), 1);
/// # Ok::<(), stackly_domain::SecretRefError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct FakeSecretStore {
    shared: Arc<Shared>,
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    hold_changed: Condvar,
}

#[derive(Debug, Default)]
struct State {
    /// The stored values. A `SecretValue` wipes itself when it drops.
    items: BTreeMap<SecretRef, SecretValue>,
    locked: bool,
    unsupported: bool,
    denied: BTreeSet<SecretRef>,
    observed: Option<StoreState>,
    fail_next: Option<SecretStoreError>,
    hold: HoldState,
    calls: Vec<SecretCall>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum HoldState {
    /// No hold armed.
    #[default]
    Idle,
    /// The next fallible call will wait.
    Armed,
    /// A call is waiting for [`SecretHold::release`].
    Holding,
}

impl FakeSecretStore {
    /// Creates an empty, unlocked store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a value without a call, to set up a test. It replaces any
    /// value already there and does not count as a call.
    ///
    /// # Panics
    ///
    /// Panics when `value` is not a valid secret value (empty, too long,
    /// or with a NUL byte).
    pub fn insert(&self, reference: &SecretRef, value: &str) {
        let value = SecretValue::new(value.to_owned())
            .expect("a fixture secret value is 1 to 65536 bytes without NUL");
        self.lock_state().items.insert(reference.clone(), value);
    }

    /// Removes a value without a call, as when a user deletes the item in
    /// Keychain Access.
    pub fn remove(&self, reference: &SecretRef) {
        self.lock_state().items.remove(reference);
    }

    /// Returns true when a value is stored for the reference.
    #[must_use]
    pub fn contains(&self, reference: &SecretRef) -> bool {
        self.lock_state().items.contains_key(reference)
    }

    /// Returns a copy of the stored value, so a test can check what a
    /// write left behind.
    #[must_use]
    pub fn stored_value(&self, reference: &SecretRef) -> Option<String> {
        self.lock_state()
            .items
            .get(reference)
            .map(|value| value.expose().to_owned())
    }

    /// Locks the store: every fallible call fails `Locked` until
    /// [`unlock`](Self::unlock).
    pub fn lock(&self) {
        self.lock_state().locked = true;
    }

    /// Unlocks the store.
    pub fn unlock(&self) {
        self.lock_state().locked = false;
    }

    /// Makes the item behind `reference` distrust this process: a read,
    /// write, or delete of it fails `AccessDenied`, with `by_user` true
    /// under [`Interaction::AllowPrompt`] (the user denied the dialog) and
    /// false under [`Interaction::Never`] (no dialog was allowed). A probe
    /// reads attributes only, so it still works.
    pub fn deny(&self, reference: &SecretRef) {
        self.lock_state().denied.insert(reference.clone());
    }

    /// Trusts this process again for `reference`.
    pub fn allow(&self, reference: &SecretRef) {
        self.lock_state().denied.remove(reference);
    }

    /// Switches the store off, like a store on an OS without an adapter:
    /// every fallible call fails `NotSupported` for macOS and the state is
    /// `Unsupported`.
    pub fn unsupported(&self) {
        self.lock_state().unsupported = true;
    }

    /// Makes the next fallible call fail with `error` after it is counted,
    /// whatever the store holds. The store does not change.
    pub fn fail_next(&self, error: SecretStoreError) {
        self.lock_state().fail_next = Some(error);
    }

    /// Makes the next fallible call wait, after it is counted, until the
    /// returned hold is released or dropped, like a Keychain call stuck
    /// behind a dialog.
    #[must_use = "the held call waits until the hold is released or dropped"]
    pub fn hold_next(&self) -> SecretHold {
        self.lock_state().hold = HoldState::Armed;
        SecretHold {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Returns every call that reached the store, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<SecretCall> {
        self.lock_state().calls.clone()
    }

    /// Returns how many calls of one method reached the store.
    #[must_use]
    pub fn count(&self, op: SecretOp) -> usize {
        self.lock_state()
            .calls
            .iter()
            .filter(|call| call.op == op)
            .count()
    }

    fn lock_state(&self) -> MutexGuard<'_, State> {
        lock(&self.shared.state)
    }

    /// Counts the call, waits on a hold, applies the injected failure, the
    /// unsupported switch, the lock, and the denial, then runs `body` on
    /// the items and records what the call observed.
    fn call<T>(
        &self,
        op: SecretOp,
        reference: &SecretRef,
        interaction: Option<Interaction>,
        body: impl FnOnce(&mut BTreeMap<SecretRef, SecretValue>) -> Result<T, SecretStoreError>,
    ) -> Result<T, SecretStoreError> {
        let mut state = self.lock_state();
        state.calls.push(SecretCall {
            op,
            reference: reference.clone(),
            interaction,
        });
        if state.hold == HoldState::Armed {
            state.hold = HoldState::Holding;
            self.shared.hold_changed.notify_all();
            while state.hold == HoldState::Holding {
                state = self
                    .shared
                    .hold_changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
        let result = if let Some(error) = state.fail_next.take() {
            Err(error)
        } else if state.unsupported {
            Err(SecretStoreError::NotSupported { os: Os::Macos })
        } else if state.locked {
            Err(SecretStoreError::Locked)
        } else if op != SecretOp::Probe && state.denied.contains(reference) {
            Err(SecretStoreError::AccessDenied {
                by_user: interaction == Some(Interaction::AllowPrompt),
            })
        } else {
            body(&mut state.items)
        };
        match result {
            Err(SecretStoreError::Locked) => state.observed = Some(StoreState::Locked),
            Err(SecretStoreError::NotSupported { .. }) => {
                state.observed = Some(StoreState::Unsupported);
            }
            Ok(_)
            | Err(
                SecretStoreError::NotFound
                | SecretStoreError::AlreadyExists
                | SecretStoreError::AccessDenied { .. },
            ) => state.observed = Some(StoreState::Available),
            Err(_) => {}
        }
        result
    }
}

impl SecretStore for FakeSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Fake
    }

    fn state(&self) -> StoreState {
        let state = self.lock_state();
        if state.unsupported {
            return StoreState::Unsupported;
        }
        state.observed.unwrap_or(StoreState::Unknown)
    }

    fn store_key(&self, reference: &SecretRef) -> String {
        format!("fake:{SECRET_SERVICE}/{}", reference.path())
    }

    fn probe(&self, reference: &SecretRef) -> Result<SecretPresence, SecretStoreError> {
        self.call(SecretOp::Probe, reference, None, |items| {
            Ok(if items.contains_key(reference) {
                SecretPresence::Present
            } else {
                SecretPresence::Missing
            })
        })
    }

    fn read(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<SecretValue, SecretStoreError> {
        self.call(SecretOp::Read, reference, Some(interaction), |items| {
            let stored = items.get(reference).ok_or(SecretStoreError::NotFound)?;
            Ok(SecretValue::new(stored.expose().to_owned())
                .expect("a stored value was a valid secret value when it was stored"))
        })
    }

    fn write(
        &self,
        reference: &SecretRef,
        value: &SecretValue,
        mode: WriteMode,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        self.call(SecretOp::Write, reference, Some(interaction), |items| {
            let exists = items.contains_key(reference);
            match (mode, exists) {
                (WriteMode::Create, true) => return Err(SecretStoreError::AlreadyExists),
                (WriteMode::Replace, false) => return Err(SecretStoreError::NotFound),
                _ => {}
            }
            let copy = SecretValue::new(value.expose().to_owned())
                .expect("a secret value stays valid when it is copied");
            items.insert(reference.clone(), copy);
            Ok(())
        })
    }

    fn delete(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        self.call(
            SecretOp::Delete,
            reference,
            Some(interaction),
            |items| match items.remove(reference) {
                Some(_) => Ok(()),
                None => Err(SecretStoreError::NotFound),
            },
        )
    }
}

/// A call held by [`FakeSecretStore::hold_next`]. Dropping it releases the
/// call, so a failed test never leaves a thread waiting.
#[derive(Debug)]
pub struct SecretHold {
    shared: Arc<Shared>,
}

impl SecretHold {
    /// Waits up to `timeout` for the held call to arrive, and returns true
    /// when it is now waiting.
    #[must_use]
    pub fn wait_until_held(&self, timeout: Duration) -> bool {
        let state = lock(&self.shared.state);
        let (state, _) = self
            .shared
            .hold_changed
            .wait_timeout_while(state, timeout, |state| state.hold != HoldState::Holding)
            .unwrap_or_else(PoisonError::into_inner);
        state.hold == HoldState::Holding
    }

    /// Lets the held call go on. A hold whose call never came is disarmed.
    pub fn release(self) {
        drop(self);
    }
}

impl Drop for SecretHold {
    fn drop(&mut self) {
        let mut state = lock(&self.shared.state);
        state.hold = HoldState::Idle;
        self.shared.hold_changed.notify_all();
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}
