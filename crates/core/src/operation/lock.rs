//! The lock table in memory (spec 0007, "Lock dan antrian"): who holds each
//! lock key. Nothing of it is ever a lock in the registry; the registry's
//! `operation_locks` rows only rebuild it after a restart (REQ-048).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_domain::{LockKey, OperationId, OperationState};
use tokio::sync::Notify;

/// Who holds a lock key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockHolder {
    /// An operation, with its state when it was last updated.
    Operation {
        /// The operation.
        id: OperationId,
        /// Its state.
        state: OperationState,
    },
    /// A short critical section outside any operation, by its name.
    Internal(&'static str),
}

/// A key that another holder already has.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("lock key {key} is held by {holder:?}")]
pub struct LockBusy {
    /// The first key that was held.
    pub key: LockKey,
    /// Who holds it.
    pub holder: LockHolder,
}

#[derive(Debug, Default)]
struct Held {
    keys: HashMap<LockKey, LockHolder>,
}

/// The lock keys held by running operations, operations in
/// `needs_recovery`, and internal holders. Cloning shares the table.
///
/// An operation takes all its keys at once or none. A queued operation
/// holds none; the dispatcher takes them when it starts the operation.
#[derive(Debug, Clone)]
pub struct LockTable {
    held: Arc<Mutex<Held>>,
    released: Arc<Notify>,
}

impl LockTable {
    pub(crate) fn new(released: Arc<Notify>) -> Self {
        Self {
            held: Arc::new(Mutex::new(Held::default())),
            released,
        }
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns who holds `key`, if anyone.
    #[must_use]
    pub fn holder(&self, key: &LockKey) -> Option<LockHolder> {
        self.held().keys.get(key).copied()
    }

    /// Takes `keys` for a short critical section outside any operation, or
    /// fails at once when one is held. Hold it for at most one registry
    /// commit and never across external I/O; queued operations do not get
    /// its keys meanwhile. Dropping the guard releases them.
    ///
    /// # Errors
    ///
    /// Returns [`LockBusy`] naming the first held key; nothing is taken.
    pub fn try_lock_internal(
        &self,
        keys: &[LockKey],
        name: &'static str,
    ) -> Result<InternalLock, LockBusy> {
        self.try_take(keys, LockHolder::Internal(name))?;
        Ok(InternalLock {
            table: self.clone(),
            keys: keys.to_vec(),
            name,
        })
    }

    /// Takes every key for `holder`, or none.
    pub(crate) fn try_take(&self, keys: &[LockKey], holder: LockHolder) -> Result<(), LockBusy> {
        let mut held = self.held();
        if let Some((key, other)) = keys
            .iter()
            .find_map(|key| held.keys.get(key).map(|other| (key, *other)))
        {
            return Err(LockBusy {
                key: key.clone(),
                holder: other,
            });
        }
        for key in keys {
            held.keys.insert(key.clone(), holder);
        }
        Ok(())
    }

    /// Records the new state of an operation on every key it holds.
    pub(crate) fn set_state(&self, id: OperationId, state: OperationState) {
        for holder in self.held().keys.values_mut() {
            if let LockHolder::Operation {
                id: owner,
                state: held,
            } = holder
                && *owner == id
            {
                *held = state;
            }
        }
    }

    /// Releases every key of an operation.
    pub(crate) fn release_operation(&self, id: OperationId) {
        self.held().keys.retain(
            |_, holder| !matches!(holder, LockHolder::Operation { id: owner, .. } if *owner == id),
        );
        self.released.notify_one();
    }

    fn release_keys(&self, keys: &[LockKey], name: &'static str) {
        let mut held = self.held();
        for key in keys {
            if held.keys.get(key) == Some(&LockHolder::Internal(name)) {
                held.keys.remove(key);
            }
        }
        drop(held);
        self.released.notify_one();
    }
}

/// Keys held by [`LockTable::try_lock_internal`] until it drops.
#[derive(Debug)]
#[must_use = "dropping the guard releases the keys at once"]
pub struct InternalLock {
    table: LockTable,
    keys: Vec<LockKey>,
    name: &'static str,
}

impl Drop for InternalLock {
    fn drop(&mut self) {
        self.table.release_keys(&self.keys, self.name);
    }
}

#[cfg(test)]
mod tests {
    use stackly_domain::InstanceId;

    use super::*;

    fn table() -> LockTable {
        LockTable::new(Arc::new(Notify::new()))
    }

    #[test]
    fn keys_are_taken_all_at_once_or_not_at_all() {
        let locks = table();
        let a = LockKey::Instance(InstanceId::generate());
        let b = LockKey::Manager;
        let first = OperationId::generate();
        locks
            .try_take(
                std::slice::from_ref(&b),
                LockHolder::Operation {
                    id: first,
                    state: OperationState::Running,
                },
            )
            .unwrap();

        let refused = locks.try_take(
            &[a.clone(), b.clone()],
            LockHolder::Operation {
                id: OperationId::generate(),
                state: OperationState::Running,
            },
        );

        assert_eq!(refused.unwrap_err().key, b);
        assert_eq!(locks.holder(&a), None, "nothing of a refused set is taken");
        locks.set_state(first, OperationState::NeedsRecovery);
        assert_eq!(
            locks.holder(&b),
            Some(LockHolder::Operation {
                id: first,
                state: OperationState::NeedsRecovery
            })
        );
        locks.release_operation(first);
        assert_eq!(locks.holder(&b), None);
    }

    #[test]
    fn an_internal_lock_fails_at_once_and_releases_on_drop() {
        let locks = table();
        let key = LockKey::Manager;
        let guard = locks
            .try_lock_internal(std::slice::from_ref(&key), "lifecycle_write")
            .unwrap();

        let again = locks.try_lock_internal(std::slice::from_ref(&key), "other");

        assert_eq!(
            again.unwrap_err().holder,
            LockHolder::Internal("lifecycle_write")
        );
        drop(guard);
        assert_eq!(locks.holder(&key), None);
    }
}
