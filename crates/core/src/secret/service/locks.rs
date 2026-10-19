//! The in memory lock per reference that keeps `put`, `delete`, and the
//! probes of one reference from overlapping.

use std::collections::BTreeSet;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use stackly_domain::SecretRef;

/// The references held now. A holder waits while another holds the same
/// reference; references are always taken in text order, so two holders
/// of several never wait on each other.
#[derive(Debug, Default)]
pub(super) struct ReferenceLocks {
    held: Mutex<BTreeSet<SecretRef>>,
    released: Condvar,
}

/// Holds references until it drops.
#[derive(Debug)]
pub(super) struct ReferenceGuard<'a> {
    locks: &'a ReferenceLocks,
    references: Vec<SecretRef>,
}

impl ReferenceLocks {
    /// Waits until `reference` is free, then holds it.
    pub(super) fn lock(&self, reference: &SecretRef) -> ReferenceGuard<'_> {
        self.lock_all(std::slice::from_ref(reference))
    }

    /// Waits until every reference is free, then holds them all, taking
    /// them one by one in text order.
    pub(super) fn lock_all(&self, references: &[SecretRef]) -> ReferenceGuard<'_> {
        let mut sorted = references.to_vec();
        sorted.sort();
        sorted.dedup();
        let mut held = self.held();
        for reference in &sorted {
            while held.contains(reference) {
                held = self
                    .released
                    .wait(held)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            held.insert(reference.clone());
        }
        ReferenceGuard {
            locks: self,
            references: sorted,
        }
    }

    fn held(&self) -> MutexGuard<'_, BTreeSet<SecretRef>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for ReferenceGuard<'_> {
    fn drop(&mut self) {
        let mut held = self.locks.held();
        for reference in &self.references {
            held.remove(reference);
        }
        drop(held);
        self.locks.released.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use super::*;

    fn reference(text: &str) -> SecretRef {
        SecretRef::parse(text).unwrap()
    }

    #[test]
    fn a_held_reference_waits_for_its_holder_and_others_do_not() {
        let locks = Arc::new(ReferenceLocks::default());
        let first = reference("local:a/b");
        let guard = locks.lock(&first);

        let other = locks.lock(&reference("local:a/c"));
        drop(other);

        let entered = Arc::new(AtomicBool::new(false));
        let waiter = {
            let locks = Arc::clone(&locks);
            let entered = Arc::clone(&entered);
            let first = first.clone();
            std::thread::spawn(move || {
                let _guard = locks.lock(&first);
                entered.store(true, Ordering::SeqCst);
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!entered.load(Ordering::SeqCst));
        drop(guard);
        waiter.join().unwrap();
        assert!(entered.load(Ordering::SeqCst));
    }

    #[test]
    fn holding_several_references_takes_each_once() {
        let locks = ReferenceLocks::default();
        let a = reference("local:a/a");
        let b = reference("local:a/b");
        let guard = locks.lock_all(&[b.clone(), a.clone(), b.clone()]);
        assert_eq!(guard.references, [a.clone(), b.clone()]);
        drop(guard);
        let _again = locks.lock_all(&[a, b]);
    }
}
