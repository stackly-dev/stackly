//! The probe runners (spec 0010, *Probe, readiness, dan health*, and K3):
//! one attempt of a `tcp`, `http`, `exec`, or `native` probe from the plan, with one
//! [`ProbeOutcome`] for every kind.
//!
//! An attempt never reads a body, never follows a redirect, and talks only
//! to the instance's own loopback endpoint or runs the runtime's own
//! executable. Every failure detail takes the same path in every runner:
//! the raw text is redacted with the [`Redactor`], cut to 256 characters,
//! and only then becomes an [`ObservationDetail`]
//! ([`ProbeOutcome::failure`]).

mod exec;
mod http;
mod native;
mod tcp;

pub use native::{Connecting, NativeConnect, NativeStream, TcpConnect};

use std::time::Duration;

use stackly_domain::{ObservationDetail, ReasonCode};
use stackly_platform::Platform;

use super::{SupervisorConfig, observation_detail};
use crate::provider::ProbeSpec;
use crate::secret::Redactor;

/// The probe succeeded.
pub const PROBE_OK: ReasonCode = ReasonCode::from_static("probe_ok");
/// [`PROBE_OK`] as a value [`ProbeOutcome::reason`] can lend out.
static PROBE_OK_REASON: ReasonCode = PROBE_OK;
/// The `tcp` or `http` connect was refused or failed.
pub const CONNECT_FAILED: ReasonCode = ReasonCode::from_static("connect_failed");
/// The attempt ran past its time limit; an `exec` probe was terminated by
/// the helper rule.
pub const TIMEOUT: ReasonCode = ReasonCode::from_static("timeout");
/// The `http` response could not be read as HTTP.
pub const PROTOCOL_ERROR: ReasonCode = ReasonCode::from_static("protocol_error");
/// The `http` status is outside `expect_status`; the detail is
/// `status <n>`.
pub const UNEXPECTED_STATUS: ReasonCode = ReasonCode::from_static("unexpected_status");
/// The `exec` probe exited outside `success_exit_codes`, or could not run;
/// the detail is the first non-empty stderr line, else `exit <code>`.
pub const CHECK_FAILED: ReasonCode = ReasonCode::from_static("check_failed");

/// What one probe attempt found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The probe succeeded (reason [`PROBE_OK`]).
    Ok,
    /// The probe failed.
    Failed {
        /// Why, such as [`CONNECT_FAILED`].
        reason: ReasonCode,
        /// What the runner saw, redacted and at most 256 characters.
        detail: Option<ObservationDetail>,
    },
}

impl ProbeOutcome {
    /// Builds a failure from the raw text a runner saw, in the one order
    /// every runner follows: redact `raw` with `redactor`, cut it to 256
    /// characters, then make the [`ObservationDetail`]. An empty text has
    /// no detail.
    #[must_use]
    pub fn failure(reason: ReasonCode, raw: &str, redactor: &Redactor) -> Self {
        Self::Failed {
            reason,
            detail: observation_detail(&redactor.redact(raw)),
        }
    }

    /// Returns the reason an observation records: [`PROBE_OK`] for a
    /// success.
    #[must_use]
    pub fn reason(&self) -> &ReasonCode {
        match self {
            Self::Ok => &PROBE_OK_REASON,
            Self::Failed { reason, .. } => reason,
        }
    }

    /// Returns the failure detail, if any.
    #[must_use]
    pub const fn detail(&self) -> Option<&ObservationDetail> {
        match self {
            Self::Ok => None,
            Self::Failed { detail, .. } => detail.as_ref(),
        }
    }

    /// Returns true for a success.
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }
}

/// What a probe attempt needs besides its plan.
#[derive(Debug, Clone, Copy)]
pub struct ProbeContext<'a> {
    /// The platform, whose process port runs `exec` probes.
    pub platform: &'a Platform,
    /// Hides known secrets in every failure detail.
    pub redactor: &'a Redactor,
    /// The supervisor constants: the HTTP header limit and the helper
    /// grace period.
    pub config: &'a SupervisorConfig,
    /// Opens the connection of a `native` probe.
    pub native: &'a dyn NativeConnect,
}

/// Runs one attempt of `probe` within `attempt_timeout`.
///
/// `tcp` and `http` share one deadline for connect, request, and status
/// line. An `exec` probe runs as a [`ChildRole::Probe`] helper with the
/// plan's argv and env, stdin from `/dev/null`, under the shorter of
/// `attempt_timeout` and its own timeout, and the helper rule applies when
/// it runs past it.
///
/// [`ChildRole::Probe`]: stackly_platform::ChildRole::Probe
pub async fn run(
    probe: &ProbeSpec,
    attempt_timeout: Duration,
    context: ProbeContext<'_>,
) -> ProbeOutcome {
    match probe {
        ProbeSpec::Tcp { target } => tcp::run(target, attempt_timeout, context.redactor).await,
        ProbeSpec::Http {
            host,
            port,
            path,
            expect_status,
        } => {
            http::run(
                http::HttpProbe {
                    host: *host,
                    port: *port,
                    path,
                    expect_status,
                },
                attempt_timeout,
                context,
            )
            .await
        }
        ProbeSpec::Exec(spec) => exec::run(spec, attempt_timeout, context).await,
        ProbeSpec::Native(spec) => native::run(spec, attempt_timeout, &context).await,
    }
}

#[cfg(test)]
mod tests {
    use stackly_platform::SecretValue;

    use super::*;

    #[test]
    fn the_reason_codes_are_the_codes_of_spec_0010() {
        let codes = [
            PROBE_OK,
            CONNECT_FAILED,
            TIMEOUT,
            PROTOCOL_ERROR,
            UNEXPECTED_STATUS,
            CHECK_FAILED,
        ];

        assert_eq!(
            codes.each_ref().map(ReasonCode::as_str),
            [
                "probe_ok",
                "connect_failed",
                "timeout",
                "protocol_error",
                "unexpected_status",
                "check_failed",
            ]
        );
    }

    #[test]
    fn a_success_reports_probe_ok_and_no_detail() {
        assert_eq!(ProbeOutcome::Ok.reason(), &PROBE_OK);
        assert_eq!(ProbeOutcome::Ok.detail(), None);
        assert!(ProbeOutcome::Ok.is_ok());
    }

    #[test]
    fn a_failure_detail_is_redacted_before_it_is_cut() {
        let redactor = Redactor::new();
        let secret = "s3cret-token-value";
        redactor.learn(&SecretValue::new(secret.to_owned()).unwrap());
        // The secret straddles character 256: cutting first would keep a
        // piece of it that the redactor no longer recognizes.
        let raw = format!("{}{secret}", "x".repeat(250));

        let outcome = ProbeOutcome::failure(CHECK_FAILED, &raw, &redactor);

        let detail = outcome.detail().unwrap().as_str();
        assert_eq!(detail.chars().count(), 254);
        assert!(detail.ends_with("x****"), "{detail}");
        assert!(!detail.contains("s3cret"));
        assert_eq!(outcome.reason(), &CHECK_FAILED);
    }

    #[test]
    fn a_long_failure_detail_is_cut_to_256_characters() {
        let outcome = ProbeOutcome::failure(PROTOCOL_ERROR, &"ü".repeat(1000), &Redactor::new());

        assert_eq!(outcome.detail().unwrap().as_str().chars().count(), 256);
    }

    #[test]
    fn an_empty_failure_text_has_no_detail() {
        let outcome = ProbeOutcome::failure(TIMEOUT, "", &Redactor::new());

        assert_eq!(
            outcome,
            ProbeOutcome::Failed {
                reason: TIMEOUT,
                detail: None
            }
        );
    }
}
