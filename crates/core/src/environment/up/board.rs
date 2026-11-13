//! `UpBoard` (spec 0046, *Operasi `environment_up`*): the progress of every
//! running Up, in memory only. The work writes it on every phase change and
//! removes its entry after the closing commit; `env.get` reads it while the
//! operation is active. It is never written to disk.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use jiff::Timestamp;
use stackly_domain::{EnvironmentId, MemberAlias, OperationId, ServiceReport};

/// One service of a running Up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveService {
    /// What the Up knows of it now, as the closing commit would store it.
    pub report: ServiceReport,
    /// The dependencies it waits for while `waiting`.
    pub waiting_on: Vec<MemberAlias>,
}

/// The progress of one running Up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveUp {
    /// The `environment_up` operation.
    pub operation: OperationId,
    /// When its `accept` step committed.
    pub started_at: Timestamp,
    /// Every service, in manifest order.
    pub services: Vec<LiveService>,
}

/// The progress table of every running Up, one for the whole daemon.
#[derive(Debug, Default)]
pub struct UpBoard {
    entries: Mutex<HashMap<EnvironmentId, LiveUp>>,
}

impl UpBoard {
    /// Creates an empty board.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the progress of the Up of `environment`, replacing what was
    /// there.
    pub fn put(&self, environment: EnvironmentId, live: LiveUp) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(environment, live);
    }

    /// Returns the progress of the Up of `environment`, if one is on the
    /// board.
    #[must_use]
    pub fn get(&self, environment: EnvironmentId) -> Option<LiveUp> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&environment)
            .cloned()
    }

    /// Removes the entry of `environment` when it belongs to `operation`,
    /// so a later Up's entry is never removed by an older one.
    pub fn remove(&self, environment: EnvironmentId, operation: OperationId) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries
            .get(&environment)
            .is_some_and(|live| live.operation == operation)
        {
            entries.remove(&environment);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_up_never_removes_the_entry_of_a_newer_one() {
        let board = UpBoard::new();
        let environment = EnvironmentId::generate();
        let older = OperationId::generate();
        let newer = OperationId::generate();
        board.put(
            environment,
            LiveUp {
                operation: newer,
                started_at: Timestamp::UNIX_EPOCH,
                services: Vec::new(),
            },
        );

        board.remove(environment, older);
        assert!(board.get(environment).is_some());

        board.remove(environment, newer);
        assert!(board.get(environment).is_none());
    }
}
