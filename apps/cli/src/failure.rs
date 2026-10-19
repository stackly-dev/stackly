//! [`CliFailure`], the one shape an error takes inside the CLI, and a
//! constructor per code the CLI makes itself (spec 0008, *Kode error area
//! cli*).
//!
//! A daemon error is kept exactly as the client decoded it. Only the CLI's
//! own codes, plus `ipc.call_timeout` and the transport codes of spec 0006,
//! are built here.

use std::time::Duration;

use stackly_protocol::codes::{cli, internal, ipc};
use stackly_protocol::{ErrorBody, ResourceKind, ResourceRef};
use uuid::Uuid;

use crate::exit::{EXIT_INTERRUPTED, exit_code};

/// An error the command ends with: the body for the envelope, the
/// `requestId` of the daemon request that failed (`None` for an error the
/// CLI found itself), and an exit code that overrides the category's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliFailure {
    /// The error, as the daemon sent it or as the CLI made it.
    pub body: Box<ErrorBody>,
    /// The `requestId` of the daemon request that produced `body`.
    pub request_id: Option<String>,
    /// Set only by [`CliFailure::interrupted`], to 130.
    pub exit_override: Option<u8>,
}

impl CliFailure {
    /// A failure the CLI found itself, with no daemon request behind it.
    #[must_use]
    pub fn local(body: ErrorBody) -> Self {
        Self {
            body: Box::new(body),
            request_id: None,
            exit_override: None,
        }
    }

    /// The daemon's error for the request `request_id`, unchanged.
    #[must_use]
    pub fn daemon(body: impl Into<Box<ErrorBody>>, request_id: Option<String>) -> Self {
        Self {
            body: body.into(),
            request_id,
            exit_override: None,
        }
    }

    /// The exit code: the override when set, else the category's.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self.exit_override {
            Some(code) => code,
            None => exit_code(self.body.category()),
        }
    }

    /// `cli.usage`: clap refused the arguments.
    #[must_use]
    pub fn usage(kind: &str, message: &str) -> Self {
        Self::local(ErrorBody::new(&cli::USAGE, message).with_detail("kind", kind))
    }

    /// `cli.invalid_environment`: `variable` holds a value the CLI refuses.
    #[must_use]
    pub fn invalid_environment(variable: &str, reason: &str) -> Self {
        Self::local(
            ErrorBody::new(
                &cli::INVALID_ENVIRONMENT,
                format!("the value of {variable} is refused: {reason}"),
            )
            .with_detail("variable", variable)
            .with_detail("reason", reason),
        )
    }

    /// `cli.platform_unavailable`: the roots or the socket path cannot be
    /// worked out.
    #[must_use]
    pub fn platform_unavailable(reason: &str, message: &str) -> Self {
        Self::local(
            ErrorBody::new(&cli::PLATFORM_UNAVAILABLE, message).with_detail("reason", reason),
        )
    }

    /// `cli.input_required`: a prompt for `input` is needed, but the CLI runs
    /// noninteractive.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "spec 0011 step 8 asks for secrets through it")
    )]
    pub fn input_required(input: &str) -> Self {
        Self::local(
            ErrorBody::new(
                &cli::INPUT_REQUIRED,
                "an answer is needed, but stackly runs noninteractive",
            )
            .with_detail("input", input)
            .with_hint("run it in a terminal without --no-input or STACKLY_NO_INPUT"),
        )
    }

    /// `cli.confirmation_required`: a confirmation is needed, but the CLI
    /// runs noninteractive and `flag` was not given.
    #[must_use]
    pub fn confirmation_required(flag: &str) -> Self {
        Self::local(
            ErrorBody::new(
                &cli::CONFIRMATION_REQUIRED,
                "a confirmation is needed, but stackly runs noninteractive",
            )
            .with_detail("flag", flag)
            .with_hint(format!("pass {flag} to confirm")),
        )
    }

    /// `cli.confirmation_declined`: the answer was not `y` or `yes`.
    #[must_use]
    pub fn confirmation_declined() -> Self {
        Self::local(ErrorBody::new(
            &cli::CONFIRMATION_DECLINED,
            "not confirmed, so nothing was done",
        ))
    }

    /// `cli.wait_timeout`: `--timeout` ran out while operation `id` was in
    /// `state`.
    #[must_use]
    pub fn wait_timeout(id: Uuid, state: &str, timeout: Duration) -> Self {
        Self::local(
            operation(
                ErrorBody::new(
                    &cli::WAIT_TIMEOUT,
                    format!("stopped waiting; operation {id} is still {state}"),
                ),
                id,
            )
            .with_detail("state", state)
            .with_hint(check_hint(id))
            .with_detail("timeoutSeconds", timeout.as_secs().to_string()),
        )
    }

    /// `cli.wait_disconnected`: the daemon did not come back within 30
    /// seconds while waiting for operation `id`.
    #[must_use]
    pub fn wait_disconnected(id: Uuid, state: &str) -> Self {
        Self::local(
            operation(
                ErrorBody::new(
                    &cli::WAIT_DISCONNECTED,
                    "the connection to the daemon did not come back within 30 seconds",
                ),
                id,
            )
            .with_detail("state", state)
            .with_hint(check_hint(id)),
        )
    }

    /// `cli.interrupted`: Ctrl+C stopped the wait for operation `id`, which
    /// keeps running. Exits 130.
    #[must_use]
    pub fn interrupted(id: Uuid, state: &str) -> Self {
        Self {
            body: Box::new(
                operation(
                    ErrorBody::new(
                        &cli::INTERRUPTED,
                        format!("stopped waiting; operation {id} is still {state}"),
                    ),
                    id,
                )
                .with_detail("state", state)
                .with_hint(check_hint(id)),
            ),
            request_id: None,
            exit_override: Some(EXIT_INTERRUPTED),
        }
    }

    /// `cli.cancel_not_applied`: operation `id` ended `state` instead of
    /// `cancelled`.
    #[must_use]
    pub fn cancel_not_applied(id: Uuid, state: &str) -> Self {
        Self::local(
            operation(
                ErrorBody::new(
                    &cli::CANCEL_NOT_APPLIED,
                    format!("the cancel did not apply: the operation is {state}"),
                ),
                id,
            )
            .with_detail("state", state),
        )
    }

    /// `cli.operation_failed`: operation `id` failed without an error.
    #[must_use]
    pub fn operation_failed(id: Uuid) -> Self {
        Self::local(operation(
            ErrorBody::new(
                &cli::OPERATION_FAILED,
                "the operation failed without a recorded error",
            ),
            id,
        ))
    }

    /// `cli.operation_cancelled`: operation `id` was cancelled.
    #[must_use]
    pub fn operation_cancelled(id: Uuid) -> Self {
        Self::local(operation(
            ErrorBody::new(&cli::OPERATION_CANCELLED, "the operation was cancelled"),
            id,
        ))
    }

    /// `cli.operation_needs_recovery`: operation `id` needs recovery without
    /// an error.
    #[must_use]
    pub fn operation_needs_recovery(id: Uuid) -> Self {
        Self::local(operation(
            ErrorBody::new(
                &cli::OPERATION_NEEDS_RECOVERY,
                "the operation needs recovery",
            ),
            id,
        ))
    }

    /// `cli.panic`: the CLI panicked. The panic message is never shown.
    #[must_use]
    pub fn panic() -> Self {
        Self::local(ErrorBody::new(
            &cli::PANIC,
            "stackly hit a bug and stopped; the panic message is not shown",
        ))
    }

    /// `ipc.call_timeout` (spec 0006): the CLI stopped waiting for `method`
    /// after `limit`, so the request may have run.
    #[must_use]
    pub fn call_timeout(
        method: &str,
        limit: Duration,
        idempotency_key: Option<&str>,
        request_id: Option<String>,
    ) -> Self {
        let body = ErrorBody::new(&ipc::CALL_TIMEOUT, "the daemon did not answer in time")
            .with_detail("method", method)
            .with_detail("seconds", limit.as_secs().to_string());
        let body = match idempotency_key {
            Some(key) => body.with_detail("idempotencyKey", key),
            None => body,
        };
        Self::daemon(body, request_id)
    }

    /// `ipc.protocol_violation` (spec 0006): the daemon answered a call with
    /// a reply of another method.
    #[must_use]
    pub fn unexpected_reply(request_id: Option<String>) -> Self {
        Self::daemon(
            ErrorBody::new(
                &ipc::PROTOCOL_VIOLATION,
                "the daemon answered with another reply",
            ),
            request_id,
        )
    }

    /// `ipc.not_connected` (spec 0006): no session exists, so nothing was
    /// sent. Only a bug in the CLI reaches it.
    #[must_use]
    pub fn not_connected() -> Self {
        Self::local(ErrorBody::new(
            &ipc::NOT_CONNECTED,
            "the cli is not connected to the daemon, so it sent nothing",
        ))
    }

    /// `internal.unexpected` (spec 0006): the async runtime does not start.
    #[must_use]
    pub fn runtime_unavailable() -> Self {
        Self::local(ErrorBody::new(
            &internal::UNEXPECTED,
            "cannot start the async runtime",
        ))
    }
}

/// How to look at an operation the CLI stopped waiting for.
fn check_hint(id: Uuid) -> String {
    format!("check it with: stackly operation inspect {id}")
}

/// `body` with the operation it is about, as `operationId` and resource.
fn operation(body: ErrorBody, id: Uuid) -> ErrorBody {
    body.with_operation_id(id).with_resource(ResourceRef {
        kind: ResourceKind::Operation,
        id: id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use stackly_protocol::ErrorCategory;
    use stackly_protocol::codes::catalog;

    use super::*;

    const OPERATION: Uuid = Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d0e);

    fn every_cli_failure() -> Vec<CliFailure> {
        vec![
            CliFailure::usage("unknown_argument", "unexpected argument '--nope' found"),
            CliFailure::invalid_environment("STACKLY_NO_INPUT", "unknown value"),
            CliFailure::platform_unavailable("home_dir_unavailable", "no home"),
            CliFailure::input_required("secret value"),
            CliFailure::confirmation_required("--yes"),
            CliFailure::confirmation_declined(),
            CliFailure::wait_timeout(OPERATION, "running", Duration::from_secs(90)),
            CliFailure::wait_disconnected(OPERATION, "unknown"),
            CliFailure::interrupted(OPERATION, "queued"),
            CliFailure::cancel_not_applied(OPERATION, "succeeded"),
            CliFailure::operation_failed(OPERATION),
            CliFailure::operation_cancelled(OPERATION),
            CliFailure::operation_needs_recovery(OPERATION),
            CliFailure::panic(),
        ]
    }

    #[test]
    fn there_is_one_constructor_per_cli_code_and_each_follows_the_catalog() {
        let made: Vec<&str> = every_cli_failure()
            .iter()
            .map(|failure| failure.body.code())
            .map(|code| catalog().find(|spec| spec.code == code).unwrap().code)
            .collect();
        let catalog: Vec<&str> = cli::ALL.iter().map(|spec| spec.code).collect();
        assert_eq!(made, catalog);
        for failure in every_cli_failure() {
            let spec = failure.body.spec().unwrap();
            assert_eq!(failure.body.category(), spec.category);
            assert!(!failure.body.retryable());
            assert_eq!(failure.request_id, None);
            for detail in failure.body.details() {
                assert!(spec.details.contains(&detail.key()), "{}", spec.code);
            }
            let message = failure.body.message();
            assert!(
                message.starts_with(|c: char| c.is_ascii_lowercase()),
                "{message}"
            );
            assert!(!message.ends_with('.'), "{message}");
        }
    }

    #[test]
    fn only_interrupted_overrides_the_exit_code_with_130() {
        for failure in every_cli_failure() {
            let expected = if failure.body.is(&cli::INTERRUPTED) {
                130
            } else {
                exit_code(failure.body.category())
            };
            assert_eq!(failure.exit_code(), expected, "{}", failure.body.code());
            assert_eq!(
                failure.exit_override.is_some(),
                failure.body.is(&cli::INTERRUPTED)
            );
        }
    }

    #[test]
    fn operation_codes_carry_the_operation_id_resource_and_state() {
        let failure = CliFailure::wait_timeout(OPERATION, "running", Duration::from_secs(90));
        assert_eq!(failure.body.operation_id(), Some(OPERATION));
        assert_eq!(
            failure.body.resource(),
            Some(&ResourceRef {
                kind: ResourceKind::Operation,
                id: OPERATION.to_string(),
            })
        );
        assert_eq!(failure.body.detail("state"), Some("running"));
        assert_eq!(failure.body.detail("timeoutSeconds"), Some("90"));
        assert_eq!(
            failure.body.hint(),
            Some(format!("check it with: stackly operation inspect {OPERATION}").as_str())
        );
        assert_eq!(failure.exit_code(), 8);
    }

    #[test]
    fn a_daemon_error_keeps_its_body_and_request_id_and_exits_by_category() {
        let body: ErrorBody = serde_json::from_value(serde_json::json!({
            "code": "test.failed",
            "category": "failed",
            "message": "a synthetic code",
            "retryable": false,
        }))
        .unwrap();
        let failure = CliFailure::daemon(body.clone(), Some("r-1".to_owned()));
        assert_eq!(*failure.body, body);
        assert_eq!(failure.body.category(), ErrorCategory::Failed);
        assert_eq!(failure.exit_code(), 7);
    }

    #[test]
    fn a_call_timeout_names_the_method_seconds_and_key_when_there_is_one() {
        let failure = CliFailure::call_timeout(
            "daemon.get_info",
            Duration::from_secs(10),
            None,
            Some("r-2".to_owned()),
        );
        assert!(failure.body.is(&ipc::CALL_TIMEOUT));
        assert_eq!(failure.body.detail("method"), Some("daemon.get_info"));
        assert_eq!(failure.body.detail("seconds"), Some("10"));
        assert_eq!(failure.body.detail("idempotencyKey"), None);
        assert_eq!(failure.request_id.as_deref(), Some("r-2"));
        let keyed =
            CliFailure::call_timeout("instance.start", Duration::from_secs(10), Some("k"), None);
        assert_eq!(keyed.body.detail("idempotencyKey"), Some("k"));
        assert_eq!(keyed.exit_code(), 8);
    }
}
