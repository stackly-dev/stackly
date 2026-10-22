//! What the supervisor's operations fail with (spec 0010, *Kode error*):
//! one [`SupervisorError`] whose variants carry exactly the `details` of
//! their catalog entry, and never a settings text value or a secret.

use stackly_domain::Os;
use stackly_domain::{
    DetailKey, ErrorCodeName, FailureItem, InstanceId, InstanceLifecycle, Pid, RegistryError,
    ResourceRevision,
};
use stackly_platform::ProcessError;

use super::codes;
use crate::instance::data_compat::DataIncompatible;
use crate::operation::{OperationError, codes as operation_codes};
use crate::secret::{SecretError, delivery::SecretInArgv, registry_code};

/// Why a bind of a `Fixed` port could not tell the port is free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    /// The OS refused the bind.
    Denied,
    /// The address is not on this Mac.
    Unavailable,
}

impl UnavailableReason {
    /// Returns the `reason` detail: `denied` or `unavailable`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
        }
    }
}

/// An endpoint a start or its early exit check found unusable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointFault {
    /// The endpoint's name.
    pub endpoint: String,
    /// The address, as text.
    pub address: String,
    /// The port.
    pub port: u16,
}

/// Why a supervisor operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SupervisorError {
    /// The data is not initialized yet (BR-008).
    #[error("the instance data is not initialized")]
    NotInitialized {
        /// The lifecycle it is in.
        lifecycle: InstanceLifecycle,
    },
    /// A stop while the data is being initialized.
    #[error("the instance is initializing")]
    InstanceInitializing,
    /// A start while the instance is stopping.
    #[error("the instance is stopping")]
    InstanceStopping,
    /// An action on a retained instance.
    #[error("the instance is retained")]
    InstanceRetained,
    /// The identity of the instance's process is not proven.
    #[error("the identity of the instance process is not proven")]
    IdentityUnproven {
        /// The process, if the registry names one.
        pid: Option<Pid>,
    },
    /// The instance lock is held, or the last observed process still runs.
    #[error("the instance data is in use by another process")]
    DataInUse {
        /// The lock file.
        lock_path: String,
        /// The process that is known to run, if any.
        pid: Option<Pid>,
    },
    /// Another process holds the endpoint's port.
    #[error("endpoint {} is in use at {}:{}", .0.endpoint, .0.address, .0.port)]
    EndpointInUse(EndpointFault),
    /// The OS refused the trial bind, or the address is not available.
    #[error("endpoint {} is {} at {}:{}", .fault.endpoint, .reason.as_code(), .fault.address, .fault.port)]
    EndpointUnavailable {
        /// The endpoint.
        fault: EndpointFault,
        /// Why the bind did not work.
        reason: UnavailableReason,
    },
    /// The port conflicts with another Stackly instance's effective port.
    #[error("endpoint {endpoint} conflicts with instance {other} on port {port}")]
    EndpointConflict {
        /// The endpoint.
        endpoint: String,
        /// The port.
        port: u16,
        /// The other instance.
        other: InstanceId,
    },
    /// No candidate for an `Auto` port was free.
    #[error("no free port for endpoint {endpoint}")]
    AutoPortUnavailable {
        /// The endpoint.
        endpoint: String,
    },
    /// The runtime or one it depends on is not installed.
    #[error("runtime {runtime_id} is {state}, not installed")]
    RuntimeUnavailable {
        /// The runtime.
        runtime_id: String,
        /// Its state, as a code.
        state: String,
    },
    /// The pinned package is missing, invalid, or has another digest.
    #[error("the provider package is unavailable: {reason}")]
    PackageUnavailable {
        /// The provider revision key.
        provider: String,
        /// Why.
        reason: String,
    },
    /// The dry run of the plan failed.
    #[error("the start plan is invalid: {reason}")]
    ConfigInvalid {
        /// The settings field or endpoint at fault, when there is one.
        field: String,
        /// Why.
        reason: String,
    },
    /// The executable is not a canonical regular file inside its runtime.
    #[error("the executable is invalid: {reason}")]
    ExecutableInvalid {
        /// Why.
        reason: String,
    },
    /// The instance folder is missing, not the user's, or not private.
    #[error("the instance folder is unavailable: {reason}")]
    DataUnavailable {
        /// The folder.
        path: String,
        /// Why.
        reason: String,
    },
    /// `preflight` answered anything but `Compatible`.
    #[error("the data does not fit the runtime: {outcome}")]
    DataIncompatible {
        /// The outcome, as a code.
        outcome: String,
        /// The data line, when known.
        data: Option<String>,
        /// The runtime line.
        runtime: String,
    },
    /// The readiness deadline passed.
    #[error("the instance was not ready within {deadline_seconds} s")]
    ReadinessTimeout {
        /// The deadline.
        deadline_seconds: u64,
    },
    /// The restart budget ran out.
    #[error("the process crashed {attempts} times; the retry budget is spent")]
    RetryBudgetExhausted {
        /// Spawns in the crash loop.
        attempts: u32,
        /// The last exit code, if it exited.
        last_exit_code: Option<i32>,
        /// The last signal, if a signal ended it.
        last_signal: Option<i32>,
    },
    /// The stop budget ran out.
    #[error("the instance did not stop within {timeout_seconds} s")]
    StopTimeout {
        /// The budget.
        timeout_seconds: u64,
    },
    /// An environment lease holds the instance, so a manual stop needs the
    /// override (BR-015).
    #[error("an environment holds the instance, so stopping it needs an override")]
    StopRequiresOverride,
    /// The stop command exited with a code it does not accept.
    #[error("the stop command exited with code {exit_code}")]
    StopCommandFailed {
        /// The exit code.
        exit_code: i32,
    },
    /// The reload action or its verification failed.
    #[error("the reload failed: {reason}")]
    ReloadFailed {
        /// Why.
        reason: String,
    },
    /// The provider lacks the capability (BR-016).
    #[error("the provider does not support {capability}")]
    CapabilityUnsupported {
        /// The capability.
        capability: String,
    },
    /// The init plan ended `failed`.
    #[error("the initialization failed: {reason}")]
    InitFailed {
        /// The failure code: `init_step_failed`, `init_step_timeout`,
        /// `init_timeout`, or `interrupted`.
        reason: String,
        /// The index of the step, when a step failed.
        step: Option<usize>,
        /// The exit code of the step, when it exited.
        exit_code: Option<i32>,
    },
    /// `posix_spawn` failed.
    #[error("the process could not be spawned ({kind})")]
    SpawnFailed {
        /// The kind of the OS error.
        kind: String,
    },
    /// The identity of the suspended child could not be captured or
    /// recorded.
    #[error("the identity of the new process could not be recorded")]
    IdentityCaptureFailed,
    /// A runtime executable carries the quarantine attribute, so nothing
    /// was spawned (spec 0018, K7).
    #[error("the runtime executable is quarantined")]
    Quarantined {
        /// The executable.
        path: String,
    },
    /// macOS killed the process before it was ready, which is how it
    /// refuses a binary it rejects (spec 0018, K7).
    #[error("macOS rejected the runtime binary and killed it with signal {signal}")]
    BinaryRejectedByOs {
        /// The executable.
        path: String,
        /// The signal that ended it.
        signal: i32,
    },
    /// The identity of the process changed, so no signal was sent.
    #[error("process {pid} no longer has the recorded identity, so it was not signalled")]
    SignalRefused {
        /// The process.
        pid: Pid,
    },
    /// The platform has no process supervision.
    #[error("process supervision is not supported on {os}")]
    ProcessUnsupported {
        /// The OS.
        os: Os,
    },
    /// An argument would carry a secret.
    #[error(transparent)]
    SecretInArgv(#[from] SecretInArgv),
    /// A secret could not be read or stored.
    #[error(transparent)]
    Secret(#[from] SecretError),
    /// The registry failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// The instance changed since the operation read it.
    #[error("instance {instance} is at revision {actual}, not {expected}")]
    RevisionMismatch {
        /// The instance.
        instance: InstanceId,
        /// The revision the operation read.
        expected: ResourceRevision,
        /// The revision now.
        actual: ResourceRevision,
    },
    /// The instance does not exist.
    #[error("instance {0} does not exist")]
    NotFound(InstanceId),
    /// A bug or an impossible state, for logs only.
    #[error("internal supervisor error: {reason}")]
    Internal {
        /// What went wrong.
        reason: String,
    },
}

impl From<DataIncompatible> for SupervisorError {
    fn from(refused: DataIncompatible) -> Self {
        Self::DataIncompatible {
            outcome: refused.outcome.as_code().to_owned(),
            data: refused.data.map(|data| data.to_string()),
            runtime: refused.runtime.to_string(),
        }
    }
}

impl SupervisorError {
    /// Maps a [`ProcessError`] of a spawn, or of any call that has no
    /// better meaning.
    #[must_use]
    pub fn from_process(error: &ProcessError) -> Self {
        match error {
            ProcessError::NotSupported { os, .. } => Self::ProcessUnsupported { os: *os },
            ProcessError::SpawnFailed { kind, .. } => Self::SpawnFailed {
                kind: format!("{kind:?}"),
            },
            ProcessError::IdentityMismatch { pid } => Self::SignalRefused { pid: *pid },
            other => Self::Internal {
                reason: other.to_string(),
            },
        }
    }

    /// Builds an [`Internal`](Self::Internal) error.
    #[must_use]
    pub fn internal(reason: impl Into<String>) -> Self {
        Self::Internal {
            reason: reason.into(),
        }
    }
}

/// Builds a detail key fixed in the source.
fn key(text: &str) -> DetailKey {
    text.parse()
        .expect("every detail key of spec 0010 is camelCase")
}

fn omitted() -> (DetailKey, String) {
    (key("omitted"), "0".to_owned())
}

fn endpoint_details(fault: &EndpointFault) -> Vec<(DetailKey, String)> {
    vec![
        (key("endpoint"), fault.endpoint.clone()),
        (key("address"), fault.address.clone()),
        (key("port"), fault.port.to_string()),
        omitted(),
    ]
}

impl OperationError for SupervisorError {
    fn code(&self) -> ErrorCodeName {
        match self {
            Self::NotInitialized { .. } => codes::NOT_INITIALIZED,
            Self::InstanceInitializing => codes::INSTANCE_INITIALIZING,
            Self::InstanceStopping => codes::INSTANCE_STOPPING,
            Self::InstanceRetained => codes::INSTANCE_RETAINED,
            Self::IdentityUnproven { .. } => codes::IDENTITY_UNPROVEN,
            Self::DataInUse { .. } => codes::DATA_IN_USE,
            Self::EndpointInUse(_) => codes::ENDPOINT_IN_USE,
            Self::EndpointUnavailable { .. } => codes::ENDPOINT_UNAVAILABLE,
            Self::EndpointConflict { .. } => codes::ENDPOINT_CONFLICT,
            Self::AutoPortUnavailable { .. } => codes::AUTO_PORT_UNAVAILABLE,
            Self::RuntimeUnavailable { .. } => codes::RUNTIME_UNAVAILABLE,
            Self::PackageUnavailable { .. } => codes::PACKAGE_UNAVAILABLE,
            Self::ConfigInvalid { .. } => codes::CONFIG_INVALID,
            Self::ExecutableInvalid { .. } => codes::EXECUTABLE_INVALID,
            Self::DataUnavailable { .. } => codes::DATA_UNAVAILABLE,
            Self::DataIncompatible { .. } => codes::DATA_INCOMPATIBLE,
            Self::ReadinessTimeout { .. } => codes::READINESS_TIMEOUT,
            Self::RetryBudgetExhausted { .. } => codes::RETRY_BUDGET_EXHAUSTED,
            Self::StopTimeout { .. } => codes::STOP_TIMEOUT,
            Self::StopRequiresOverride => codes::STOP_REQUIRES_OVERRIDE,
            Self::StopCommandFailed { .. } => codes::STOP_COMMAND_FAILED,
            Self::ReloadFailed { .. } => codes::RELOAD_FAILED,
            Self::CapabilityUnsupported { .. } => codes::CAPABILITY_UNSUPPORTED,
            Self::InitFailed { .. } => codes::INIT_FAILED,
            Self::SpawnFailed { .. } => codes::SPAWN_FAILED,
            Self::IdentityCaptureFailed => codes::IDENTITY_CAPTURE_FAILED,
            Self::Quarantined { .. } => crate::runtime::codes::QUARANTINED,
            Self::BinaryRejectedByOs { .. } => crate::runtime::codes::BINARY_REJECTED_BY_OS,
            Self::SignalRefused { .. } => codes::SIGNAL_REFUSED,
            Self::ProcessUnsupported { .. } => codes::PROCESS_UNSUPPORTED,
            Self::SecretInArgv(_) => crate::secret::codes::DELIVERY_REJECTED,
            Self::Secret(error) => error.code(),
            Self::Registry(error) => registry_code(error),
            Self::RevisionMismatch { .. } => operation_codes::REVISION_MISMATCH,
            Self::NotFound(_) => operation_codes::NOT_FOUND,
            Self::Internal { .. } => operation_codes::INTERNAL_UNEXPECTED,
        }
    }

    fn retryable(&self) -> bool {
        matches!(
            self,
            Self::EndpointUnavailable { .. } | Self::AutoPortUnavailable { .. }
        )
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        match self {
            Self::NotInitialized { lifecycle } => {
                vec![(key("lifecycle"), lifecycle.as_code().to_owned())]
            }
            Self::IdentityUnproven { pid } => pid
                .map(|pid| (key("pid"), pid.to_string()))
                .into_iter()
                .collect(),
            Self::DataInUse { lock_path, pid } => {
                let mut details = vec![(key("lockPath"), lock_path.clone())];
                details.extend(pid.map(|pid| (key("pid"), pid.to_string())));
                details
            }
            Self::EndpointInUse(fault) => endpoint_details(fault),
            Self::EndpointUnavailable { fault, reason } => {
                let mut details = endpoint_details(fault);
                details.insert(3, (key("reason"), reason.as_code().to_owned()));
                details
            }
            Self::EndpointConflict {
                endpoint,
                port,
                other,
            } => vec![
                (key("endpoint"), endpoint.clone()),
                (key("port"), port.to_string()),
                (key("otherInstance"), other.to_string()),
                omitted(),
            ],
            Self::AutoPortUnavailable { endpoint } => vec![(key("endpoint"), endpoint.clone())],
            Self::RuntimeUnavailable { runtime_id, state } => vec![
                (key("runtimeId"), runtime_id.clone()),
                (key("state"), state.clone()),
            ],
            Self::PackageUnavailable { provider, reason } => vec![
                (key("provider"), provider.clone()),
                (key("reason"), reason.clone()),
            ],
            Self::ConfigInvalid { field, reason } => vec![
                (key("field"), field.clone()),
                (key("reason"), reason.clone()),
            ],
            Self::ExecutableInvalid { reason } => vec![(key("reason"), reason.clone())],
            Self::DataUnavailable { path, reason } => {
                vec![(key("path"), path.clone()), (key("reason"), reason.clone())]
            }
            Self::DataIncompatible {
                outcome,
                data,
                runtime,
            } => {
                let mut details = vec![(key("outcome"), outcome.clone())];
                details.extend(data.clone().map(|data| (key("data"), data)));
                details.push((key("runtime"), runtime.clone()));
                details
            }
            Self::ReadinessTimeout { deadline_seconds } => {
                vec![(key("deadlineSeconds"), deadline_seconds.to_string())]
            }
            Self::RetryBudgetExhausted {
                attempts,
                last_exit_code,
                last_signal,
            } => {
                let mut details = vec![(key("attempts"), attempts.to_string())];
                details.extend(last_exit_code.map(|code| (key("lastExitCode"), code.to_string())));
                details.extend(last_signal.map(|signal| (key("lastSignal"), signal.to_string())));
                details
            }
            Self::StopTimeout { timeout_seconds } => {
                vec![(key("timeoutSeconds"), timeout_seconds.to_string())]
            }
            Self::StopCommandFailed { exit_code } => {
                vec![(key("exitCode"), exit_code.to_string())]
            }
            Self::ReloadFailed { reason } => vec![(key("reason"), reason.clone())],
            Self::CapabilityUnsupported { capability } => {
                vec![(key("capability"), capability.clone())]
            }
            Self::InitFailed {
                reason,
                step,
                exit_code,
            } => {
                let mut details = vec![(key("reason"), reason.clone())];
                details.extend(step.map(|step| (key("step"), step.to_string())));
                details.extend(exit_code.map(|code| (key("exitCode"), code.to_string())));
                details
            }
            Self::SpawnFailed { kind } => vec![(key("kind"), kind.clone())],
            Self::Quarantined { path } => vec![(key("path"), path.clone())],
            Self::BinaryRejectedByOs { path, signal } => vec![
                (key("path"), path.clone()),
                (key("signal"), signal.to_string()),
            ],
            Self::SignalRefused { pid } => vec![(key("pid"), pid.to_string())],
            Self::ProcessUnsupported { os } => vec![(key("os"), os.to_string())],
            Self::Secret(error) => OperationError::details(error),
            Self::SecretInArgv(error) => vec![(key("reason"), error.reason.as_code().to_owned())],
            Self::RevisionMismatch {
                expected, actual, ..
            } => vec![
                (key("expected"), expected.to_string()),
                (key("actual"), actual.to_string()),
            ],
            Self::InstanceInitializing
            | Self::StopRequiresOverride
            | Self::InstanceStopping
            | Self::InstanceRetained
            | Self::IdentityCaptureFailed
            | Self::Registry(_)
            | Self::NotFound(_)
            | Self::Internal { .. } => Vec::new(),
        }
    }

    fn items(&self) -> Vec<FailureItem> {
        match self {
            Self::Secret(error) => OperationError::items(error),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fault() -> EndpointFault {
        EndpointFault {
            endpoint: "http".to_owned(),
            address: "127.0.0.1".to_owned(),
            port: 18080,
        }
    }

    #[test]
    fn a_secret_in_argv_names_the_rule_it_broke_and_a_secret_error_delegates_its_items() {
        use crate::secret::NeedsSecret;
        use crate::secret::delivery::SecretInArgvReason;
        use stackly_domain::{NeedsSecretItem, NeedsSecretReason, SecretRef};

        let argv = SupervisorError::SecretInArgv(SecretInArgv {
            index: 1,
            field: "adminPassword".parse().expect("a setting name"),
            reason: SecretInArgvReason::AssignsValue,
        });
        let details: Vec<_> = argv
            .details()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        assert_eq!(details, [("reason".to_owned(), "assigns_value".to_owned())]);

        let reference = SecretRef::parse("local:erp/postgres/admin").expect("a reference");
        let needs = SupervisorError::Secret(SecretError::NeedsSecret {
            needs: NeedsSecret::new(vec![NeedsSecretItem::new(
                reference,
                NeedsSecretReason::StoreLocked,
                false,
            )]),
        });
        let items = OperationError::items(&needs);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind().as_str(), "reference");
        assert!(OperationError::items(&argv).is_empty());
    }

    #[test]
    fn an_endpoint_in_use_names_the_endpoint_address_and_port() {
        let error = SupervisorError::EndpointInUse(fault());

        assert_eq!(error.code(), codes::ENDPOINT_IN_USE);
        let details: Vec<_> = error
            .details()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        assert_eq!(
            details,
            [
                ("endpoint".to_owned(), "http".to_owned()),
                ("address".to_owned(), "127.0.0.1".to_owned()),
                ("port".to_owned(), "18080".to_owned()),
                ("omitted".to_owned(), "0".to_owned()),
            ]
        );
    }

    #[test]
    fn an_unavailable_endpoint_puts_the_reason_before_omitted() {
        let error = SupervisorError::EndpointUnavailable {
            fault: fault(),
            reason: UnavailableReason::Denied,
        };

        let keys: Vec<_> = error
            .details()
            .into_iter()
            .map(|(key, _)| key.to_string())
            .collect();
        assert_eq!(keys, ["endpoint", "address", "port", "reason", "omitted"]);
        assert!(error.retryable());
    }

    #[test]
    fn every_code_is_listed_once_and_follows_the_catalog_names() {
        let mut seen = std::collections::BTreeSet::new();
        for code in codes::ALL {
            assert!(seen.insert(code.to_string()), "{code} is listed twice");
            assert!(
                code.to_string().starts_with("supervisor.")
                    || code.to_string().starts_with("process."),
                "{code}"
            );
        }
    }

    #[test]
    fn messages_are_lowercase_without_a_trailing_period() {
        let errors = [
            SupervisorError::InstanceStopping,
            SupervisorError::StopTimeout {
                timeout_seconds: 10,
            },
            SupervisorError::IdentityCaptureFailed,
        ];
        for error in errors {
            let text = error.to_string();
            assert!(
                text.chars().next().is_some_and(char::is_lowercase),
                "{text}"
            );
            assert!(!text.ends_with('.'), "{text}");
        }
    }
}
