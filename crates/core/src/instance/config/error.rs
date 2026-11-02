//! The errors of a config change as operation errors (spec 0029, *Kode
//! error baru*): the three `config.*` codes, plus the create and
//! supervisor errors a change shares with create and start.

use stackly_domain::{ConfigRevisionNumber, DetailKey, ErrorCodeName, FailureItem, Rule};

use super::codes;
use crate::instance::create::CreateError;
use crate::operation::OperationError;
use crate::supervisor::SupervisorError;

fn key(text: &str) -> DetailKey {
    text.parse()
        .expect("every detail key of spec 0029 is camelCase")
}

/// Why a config change, an endpoint change, or a rename is refused or fails.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// `baseRevision` is not the active revision (`config.revision_conflict`).
    #[error("the configuration changed since revision {base} was read")]
    RevisionConflict {
        /// The revision the caller read.
        base: ConfigRevisionNumber,
        /// The active revision now.
        active: ConfigRevisionNumber,
    },
    /// A roll back with no last known good other than the active revision
    /// (`config.no_last_known_good`).
    #[error("no other revision has reached ready yet")]
    NoLastKnownGood,
    /// A restore of a revision the history no longer holds
    /// (`resource.not_found`).
    #[error("revision {number} is not in the history")]
    RevisionNotFound {
        /// The revision asked for.
        number: ConfigRevisionNumber,
    },
    /// A validation, name, or port error shared with create.
    #[error(transparent)]
    Create(#[from] CreateError),
    /// A supervisor error: lifecycle refusals, drift, reload and restart
    /// failures, and registry failures.
    #[error(transparent)]
    Supervisor(#[from] SupervisorError),
}

impl OperationError for ConfigError {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::RevisionConflict { .. } => codes::REVISION_CONFLICT,
            Self::NoLastKnownGood => codes::NO_LAST_KNOWN_GOOD,
            Self::RevisionNotFound { .. } => crate::operation::codes::NOT_FOUND,
            Self::Create(error) => OperationError::code(error),
            Self::Supervisor(error) => OperationError::code(error),
        }
    }

    fn retryable(&self) -> bool {
        match self {
            Self::RevisionConflict { .. }
            | Self::NoLastKnownGood
            | Self::RevisionNotFound { .. } => false,
            Self::Create(error) => OperationError::retryable(error),
            Self::Supervisor(error) => OperationError::retryable(error),
        }
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        match self {
            Self::RevisionConflict { base, active } => vec![
                (key("baseRevision"), base.to_string()),
                (key("activeRevision"), active.to_string()),
            ],
            Self::NoLastKnownGood => Vec::new(),
            Self::RevisionNotFound { .. } => Vec::new(),
            Self::Create(error) => OperationError::details(error),
            Self::Supervisor(error) => OperationError::details(error),
        }
    }

    fn items(&self) -> Vec<FailureItem> {
        match self {
            Self::Create(error) => OperationError::items(error),
            Self::Supervisor(error) => OperationError::items(error),
            _ => Vec::new(),
        }
    }

    fn rule(&self) -> Option<Rule> {
        match self {
            Self::Supervisor(error) => OperationError::rule(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(n: u64) -> ConfigRevisionNumber {
        ConfigRevisionNumber::new(n).unwrap()
    }

    #[test]
    fn a_stale_base_revision_names_both_revisions() {
        let error = ConfigError::RevisionConflict {
            base: number(2),
            active: number(3),
        };

        assert_eq!(error.code().to_string(), "config.revision_conflict");
        assert!(!error.retryable());
        let details: Vec<(String, String)> = error
            .details()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        assert_eq!(
            details,
            [
                ("baseRevision".to_owned(), "2".to_owned()),
                ("activeRevision".to_owned(), "3".to_owned())
            ]
        );
    }

    #[test]
    fn wrapped_errors_keep_their_own_code() {
        let drift = ConfigError::from(SupervisorError::ConfigDrift {
            files: vec!["fake.conf".to_owned()],
        });
        assert_eq!(drift.code().to_string(), "config.drift_detected");

        let taken = ConfigError::from(CreateError::NameTaken {
            existing: stackly_domain::InstanceId::generate(),
        });
        assert_eq!(taken.code().to_string(), "instance.name_taken");

        assert_eq!(
            ConfigError::NoLastKnownGood.code().to_string(),
            "config.no_last_known_good"
        );
    }
}
