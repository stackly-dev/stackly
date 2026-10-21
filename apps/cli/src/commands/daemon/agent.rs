//! The CLI's side of the background agent (spec 0015): the port calls under
//! their time limit, the state read with [`classify`], the `AgentReport`
//! that every daemon command prints, the failure each state and each macOS
//! error gets, and the text of the recovery steps.
//!
//! The only calls that change the registration are [`AgentEnv::act`] with
//! `Register` (from `install`) and `Relaunch` (from `start` and `restart`);
//! everything else here only reads the status.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use stackly_platform::bundle::{AppBundle, DAEMON_LABEL};
use stackly_platform::{
    AgentAssessment, AgentInput, AgentOp, AgentOsErrorKind, AgentProbe, BackgroundAgent,
    BackgroundAgentError, BackgroundAgentState, Clock, ProbeOutcome, RecoveryStep,
    UnavailableReason, classify,
};
use stackly_protocol::Timestamp;

use super::wait::{Interrupt, LaunchRecordSource};
use crate::failure::CliFailure;

/// How long one call on the port may take before the CLI stops waiting.
pub const AGENT_CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// What the daemon commands know about the agent: the port, the bundle the
/// CLI runs from, the launch record, and the clock.
#[derive(Clone)]
pub struct AgentEnv {
    port: Arc<dyn BackgroundAgent>,
    bundle: Option<AppBundle>,
    executable: PathBuf,
    records: Arc<dyn LaunchRecordSource>,
    root_override: bool,
    clock: Arc<dyn Clock>,
    call_timeout: Duration,
    /// Test only: run port calls on the calling task. A paused tokio clock
    /// jumps ahead while a blocking thread works, which would fire the
    /// time limit of a call that is only slow in the test's imagination.
    #[cfg(test)]
    inline: bool,
}

impl std::fmt::Debug for AgentEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentEnv")
            .field("bundle", &self.bundle)
            .field("executable", &self.executable)
            .field("root_override", &self.root_override)
            .finish_non_exhaustive()
    }
}

/// What a change call ended with.
#[derive(Debug)]
pub struct Acted {
    /// The state after the call, read with `just_registered` set.
    pub assessment: AgentAssessment,
    /// The error the command ends with, if any.
    pub failure: Option<CliFailure>,
}

/// How one port call ended.
enum CallEnd {
    Done(Result<AgentProbe, BackgroundAgentError>),
    TimedOut,
}

impl AgentEnv {
    /// Bundles what the commands need.
    #[must_use]
    pub fn new(
        port: Arc<dyn BackgroundAgent>,
        executable: PathBuf,
        records: Arc<dyn LaunchRecordSource>,
        root_override: bool,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            port,
            bundle: AppBundle::enclosing(&executable),
            executable,
            records,
            root_override,
            clock,
            call_timeout: AGENT_CALL_TIMEOUT,
            #[cfg(test)]
            inline: false,
        }
    }

    /// This environment with another time limit for a port call.
    #[cfg(test)]
    #[must_use]
    pub fn with_call_timeout(mut self, call_timeout: Duration) -> Self {
        self.call_timeout = call_timeout;
        self
    }

    /// This environment running port calls on the calling task.
    #[cfg(test)]
    #[must_use]
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    /// Whether `STACKLY_ROOT` is set.
    #[must_use]
    pub const fn root_override(&self) -> bool {
        self.root_override
    }

    /// The launch record.
    #[must_use]
    pub fn records(&self) -> &dyn LaunchRecordSource {
        &*self.records
    }

    /// The wall clock.
    #[must_use]
    pub fn clock(&self) -> &dyn Clock {
        &*self.clock
    }

    /// The bundle folder the CLI runs from, as text.
    #[must_use]
    pub fn bundle_path(&self) -> Option<String> {
        self.bundle
            .as_ref()
            .map(|bundle| bundle.root().to_string_lossy().into_owned())
    }

    /// Reads the status from the OS. With `STACKLY_ROOT` set the OS is not
    /// asked at all.
    pub async fn read_status(&self) -> ProbeOutcome {
        if self.root_override {
            return ProbeOutcome::NotChecked;
        }
        match self.call(AgentOp::Status).await {
            CallEnd::Done(Ok(probe)) => ProbeOutcome::Probe(probe),
            CallEnd::Done(Err(BackgroundAgentError::Os { code, .. })) => {
                ProbeOutcome::Failed { code: Some(code) }
            }
            CallEnd::Done(Err(_)) => ProbeOutcome::Unsupported,
            CallEnd::TimedOut => ProbeOutcome::Failed { code: None },
        }
    }

    /// Maps `probe` to a state with the launch record as it is now.
    #[must_use]
    pub fn assess(&self, probe: &ProbeOutcome, just_registered: bool) -> AgentAssessment {
        classify(&AgentInput {
            root_override: self.root_override,
            probe: probe.clone(),
            launch_record: self.records.exists(),
            just_registered,
        })
    }

    /// Reads the status and maps it, for a command that reads before it
    /// acts. Also returns the probe, which the failures need.
    pub async fn status(&self) -> (ProbeOutcome, AgentAssessment) {
        let probe = self.read_status().await;
        let assessment = self.assess(&probe, false);
        (probe, assessment)
    }

    /// The report of `assessment`. `daemon_up` false turns the recovery of
    /// an `enabled` agent into `start`.
    #[must_use]
    pub fn report(&self, assessment: &AgentAssessment, daemon_up: bool) -> AgentReport {
        let recovery = if assessment.state == BackgroundAgentState::Enabled && !daemon_up {
            RecoveryStep::Start
        } else {
            assessment.recovery
        };
        AgentReport {
            state: assessment.state.as_code(),
            reason: assessment.reason.map(UnavailableReason::as_code),
            recovery: recovery.as_code(),
            label: DAEMON_LABEL,
            bundle_path: self.bundle_path(),
            last_launched_at: self.records.last_launched_at().map(Timestamp::from_jiff),
        }
    }

    /// The failure a state that is not `enabled` stops a command with, when
    /// `probe` is what produced `assessment`. `None` for `enabled`.
    #[must_use]
    pub fn state_failure(
        &self,
        assessment: &AgentAssessment,
        probe: &ProbeOutcome,
    ) -> Option<CliFailure> {
        match assessment.state {
            BackgroundAgentState::Enabled => None,
            BackgroundAgentState::NotRegistered => Some(CliFailure::agent_not_registered()),
            BackgroundAgentState::RequiresApproval => Some(CliFailure::agent_requires_approval()),
            BackgroundAgentState::Disabled => Some(CliFailure::agent_disabled()),
            BackgroundAgentState::NotFound => {
                Some(CliFailure::agent_not_found(&self.bundle_text(probe)))
            }
            BackgroundAgentState::Unavailable => Some(self.unavailable_failure(assessment, probe)),
        }
    }

    /// Runs one change call (`Register` or `Relaunch`) and maps what came
    /// back with the table *Hasil pendaftaran*. Ctrl+C that arrived during
    /// the call is looked at only after the call ended, so a relaunch is
    /// never cut between its two steps.
    pub async fn act(&self, op: AgentOp, interrupt: &mut dyn Interrupt) -> Acted {
        debug_assert!(matches!(op, AgentOp::Register | AgentOp::Relaunch));
        let end = self.call(op).await;
        let mut acted = self.settle(op, end).await;
        if acted.failure.is_none() && interrupt.pending() {
            acted.failure = Some(CliFailure::interrupted_at("agent_call"));
        }
        acted
    }

    async fn settle(&self, op: AgentOp, end: CallEnd) -> Acted {
        let probe = match end {
            CallEnd::TimedOut => {
                let probe = ProbeOutcome::Failed { code: None };
                return Acted {
                    assessment: self.assess(&probe, false),
                    failure: Some(CliFailure::agent_call_timeout(op, self.call_timeout)),
                };
            }
            CallEnd::Done(Ok(probe)) => ProbeOutcome::Probe(probe),
            CallEnd::Done(Err(BackgroundAgentError::Os {
                kind,
                domain,
                code,
                message,
                ..
            })) => match kind {
                AgentOsErrorKind::AlreadyRegistered => self.read_status().await,
                AgentOsErrorKind::LaunchDenied | AgentOsErrorKind::MustBeEnabled => {
                    return self.denied().await;
                }
                AgentOsErrorKind::InvalidSignature => {
                    return self
                        .failed_with(CliFailure::agent_invalid_signature(code))
                        .await;
                }
                AgentOsErrorKind::NotFound => {
                    let failure =
                        CliFailure::agent_not_found(&self.bundle_path().unwrap_or_default());
                    return self.failed_with(failure).await;
                }
                AgentOsErrorKind::Other => {
                    let failure =
                        CliFailure::agent_os_error(op, &message, Some(&domain), Some(code));
                    return self.failed_with(failure).await;
                }
            },
            CallEnd::Done(Err(error)) => {
                let failure = CliFailure::agent_status_unavailable(op, &error.to_string(), None);
                return Acted {
                    assessment: self.assess(&ProbeOutcome::Unsupported, false),
                    failure: Some(failure),
                };
            }
        };
        let assessment = self.assess(&probe, true);
        let failure = self.after_action_failure(&assessment, &probe, op);
        Acted {
            assessment,
            failure,
        }
    }

    /// The state read again after the user's permission got in the way:
    /// `requires_approval` or `disabled` by the launch record, and
    /// `requires_approval` when the status cannot be read.
    async fn denied(&self) -> Acted {
        let probe = self.read_status().await;
        let assessment = self.assess(&probe, false);
        let failure = match assessment.state {
            BackgroundAgentState::Disabled => CliFailure::agent_disabled(),
            _ => CliFailure::agent_requires_approval(),
        };
        Acted {
            assessment,
            failure: Some(failure),
        }
    }

    /// An error that stops the command, with the state read for the report.
    async fn failed_with(&self, failure: CliFailure) -> Acted {
        let probe = self.read_status().await;
        Acted {
            assessment: self.assess(&probe, false),
            failure: Some(failure),
        }
    }

    /// The failure of a state read right after a change call.
    fn after_action_failure(
        &self,
        assessment: &AgentAssessment,
        probe: &ProbeOutcome,
        op: AgentOp,
    ) -> Option<CliFailure> {
        match assessment.state {
            BackgroundAgentState::Enabled => None,
            BackgroundAgentState::NotRegistered => Some(CliFailure::agent_os_error(
                op,
                "macOS did not keep the registration",
                None,
                None,
            )),
            BackgroundAgentState::Unavailable
                if assessment.reason == Some(UnavailableReason::StatusUnrecognized) =>
            {
                Some(CliFailure::agent_os_error(
                    op,
                    "macOS did not keep the registration",
                    None,
                    None,
                ))
            }
            _ => self.state_failure(assessment, probe),
        }
    }

    fn unavailable_failure(
        &self,
        assessment: &AgentAssessment,
        probe: &ProbeOutcome,
    ) -> CliFailure {
        match assessment.reason {
            Some(UnavailableReason::RootOverride) => CliFailure::agent_root_override(),
            Some(UnavailableReason::NotBundled) => {
                CliFailure::agent_not_bundled(&self.executable.to_string_lossy())
            }
            Some(UnavailableReason::HelperMissing) => {
                CliFailure::agent_helper_missing(&self.bundle_text(probe))
            }
            _ => match probe {
                ProbeOutcome::Failed { code: None } => {
                    CliFailure::agent_call_timeout(AgentOp::Status, self.call_timeout)
                }
                ProbeOutcome::Failed { code } => CliFailure::agent_status_unavailable(
                    AgentOp::Status,
                    "macOS could not report the status of the background agent",
                    *code,
                ),
                ProbeOutcome::Probe(AgentProbe::Registration {
                    registration: stackly_platform::AgentRegistration::Unrecognized(raw),
                    ..
                }) => CliFailure::agent_status_unavailable(
                    AgentOp::Status,
                    "macOS reported a status this stackly does not know",
                    Some(*raw),
                ),
                _ => CliFailure::agent_status_unavailable(
                    AgentOp::Status,
                    "the background agent is not available on this system",
                    None,
                ),
            },
        }
    }

    fn bundle_text(&self, probe: &ProbeOutcome) -> String {
        match probe {
            ProbeOutcome::Probe(AgentProbe::Registration { bundle, .. })
            | ProbeOutcome::Probe(AgentProbe::HelperMissing { bundle }) => {
                bundle.to_string_lossy().into_owned()
            }
            _ => self.bundle_path().unwrap_or_default(),
        }
    }

    /// One call on the port, on a blocking thread, under
    /// [`AGENT_CALL_TIMEOUT`]. A call that hangs cannot be cancelled; the
    /// thread is left behind and the process leaves without waiting for it.
    async fn call(&self, op: AgentOp) -> CallEnd {
        let port = Arc::clone(&self.port);
        let work = move || match op {
            AgentOp::Status => port.status(),
            AgentOp::Register => port.register(),
            AgentOp::Unregister => port.unregister(),
            AgentOp::Relaunch => port.relaunch(),
        };
        #[cfg(test)]
        if self.inline {
            return CallEnd::Done(work());
        }
        let task = tokio::task::spawn_blocking(work);
        match tokio::time::timeout(self.call_timeout, task).await {
            Ok(Ok(result)) => CallEnd::Done(result),
            Ok(Err(_)) => CallEnd::Done(Err(BackgroundAgentError::Os {
                op,
                kind: AgentOsErrorKind::Other,
                domain: "stackly".to_owned(),
                code: 0,
                message: "the call to macOS failed inside stackly".to_owned(),
            })),
            Err(_) => CallEnd::TimedOut,
        }
    }
}

/// The agent block of `--json` and of `daemon status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReport {
    /// The product state, such as `requires_approval`.
    pub state: &'static str,
    /// Why the state is `unavailable`, else null.
    pub reason: Option<&'static str>,
    /// The next step, such as `register`.
    pub recovery: &'static str,
    /// The launchd label of the agent.
    pub label: &'static str,
    /// The bundle this CLI runs from, or null outside a bundle.
    pub bundle_path: Option<String>,
    /// When launchd last started the daemon, from the launch record.
    pub last_launched_at: Option<Timestamp>,
}

/// The text of a recovery step, from the table *Langkah pemulihan CLI*;
/// `None` for `none` and for a step this build does not know.
#[must_use]
pub fn recovery_text(step: &str) -> Option<&'static str> {
    match step {
        "start" => Some(START_TEXT),
        "register" => Some("install it with stackly daemon install"),
        "approve_in_login_items" | "enable_in_login_items" => Some(LOGIN_ITEMS_TEXT),
        "move_or_reinstall_app" => {
            Some("move Stackly to the Applications folder or install it again")
        }
        "use_bundled_cli" => Some(
            "use the stackly inside Stackly.app (Stackly.app/Contents/MacOS/stackly-cli, or ~/.local/bin/stackly after installing the command line tool), or start stacklyd yourself",
        ),
        "manual_daemon_for_override" => Some("start stacklyd yourself for STACKLY_ROOT"),
        "build_with_xtask" => Some("build the app with cargo xtask build-app"),
        "check_again" => Some(
            "try again; if it keeps failing, check the background service in Stackly's Diagnostics",
        ),
        _ => None,
    }
}

/// The `start` step.
pub const START_TEXT: &str = "start it with stackly daemon start";
/// The text of both Login Items steps.
pub const LOGIN_ITEMS_TEXT: &str =
    "turn on Stackly in System Settings > General > Login Items & Extensions";

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use stackly_platform::{AgentProbe, AgentRegistration};
    use stackly_protocol::codes::cli;

    use super::super::testing::{AfterInterrupt, Rig, ScriptedRecords};
    use super::super::wait::FileRecords;
    use super::*;

    /// A port whose calls take longer than the test's time limit.
    struct Slow;

    impl BackgroundAgent for Slow {
        fn status(&self) -> Result<AgentProbe, BackgroundAgentError> {
            std::thread::sleep(Duration::from_millis(400));
            Ok(AgentProbe::NotBundled {
                executable: PathBuf::new(),
            })
        }

        fn register(&self) -> Result<AgentProbe, BackgroundAgentError> {
            self.status()
        }

        fn unregister(&self) -> Result<AgentProbe, BackgroundAgentError> {
            self.status()
        }

        fn relaunch(&self) -> Result<AgentProbe, BackgroundAgentError> {
            self.status()
        }
    }

    fn slow_env() -> AgentEnv {
        AgentEnv::new(
            Arc::new(Slow),
            PathBuf::from(super::super::testing::EXECUTABLE),
            Arc::new(FileRecords::new(None)),
            false,
            Arc::new(stackly_platform::SystemClock),
        )
        .with_call_timeout(Duration::from_millis(50))
    }

    #[tokio::test]
    async fn a_status_call_that_hangs_reads_as_a_failed_status_and_a_timeout_failure() {
        let env = slow_env();

        let (probe, assessment) = env.status().await;

        assert_eq!(probe, ProbeOutcome::Failed { code: None });
        assert_eq!(assessment.state, BackgroundAgentState::Unavailable);
        assert_eq!(assessment.reason, Some(UnavailableReason::StatusFailed));
        let failure = env.state_failure(&assessment, &probe).expect("a failure");
        assert!(failure.body.is(&cli::AGENT_CALL_TIMEOUT));
        assert_eq!(failure.body.detail("op"), Some("status"));
        assert_eq!(failure.exit_code(), 8);
    }

    #[tokio::test]
    async fn a_relaunch_that_hangs_ends_with_the_timeout_failure() {
        let env = slow_env();

        let acted = env
            .act(AgentOp::Relaunch, &mut AfterInterrupt::never())
            .await;

        let failure = acted.failure.expect("a timeout");
        assert!(failure.body.is(&cli::AGENT_CALL_TIMEOUT));
        assert_eq!(failure.body.detail("op"), Some("relaunch"));
        assert_eq!(
            crate::session::human_hints(&failure.body, None).hints,
            [
                "check with stackly daemon status; if it says not registered, run stackly daemon install"
            ]
        );
    }

    #[test]
    fn the_report_turns_the_recovery_of_an_enabled_agent_into_start_when_no_daemon_answers() {
        let rig = Rig::new(ScriptedRecords::none(), false);
        rig.fake.set_registration(AgentRegistration::Enabled);
        let assessment = rig.env.assess(
            &ProbeOutcome::Probe(AgentProbe::Registration {
                registration: AgentRegistration::Enabled,
                bundle: PathBuf::from("/Applications/Stackly.app"),
            }),
            false,
        );

        assert_eq!(rig.env.report(&assessment, true).recovery, "none");
        let down = rig.env.report(&assessment, false);
        assert_eq!(down.recovery, "start");
        assert_eq!(down.state, "enabled");
        assert_eq!(down.label, DAEMON_LABEL);
        assert_eq!(
            down.bundle_path.as_deref(),
            Some("/Applications/Stackly.app")
        );
    }

    #[tokio::test]
    async fn with_a_root_override_the_os_is_never_asked() {
        let rig = Rig::new(ScriptedRecords::none(), true);

        let (probe, assessment) = rig.env.status().await;

        assert_eq!(probe, ProbeOutcome::NotChecked);
        assert_eq!(assessment.reason, Some(UnavailableReason::RootOverride));
        assert!(rig.fake.calls().is_empty());
    }

    #[test]
    fn every_recovery_step_but_none_has_its_text_from_the_table() {
        for (step, expected) in [
            ("start", "start it with stackly daemon start"),
            ("register", "install it with stackly daemon install"),
            (
                "approve_in_login_items",
                "turn on Stackly in System Settings > General > Login Items & Extensions",
            ),
            (
                "enable_in_login_items",
                "turn on Stackly in System Settings > General > Login Items & Extensions",
            ),
            (
                "move_or_reinstall_app",
                "move Stackly to the Applications folder or install it again",
            ),
            (
                "manual_daemon_for_override",
                "start stacklyd yourself for STACKLY_ROOT",
            ),
            (
                "build_with_xtask",
                "build the app with cargo xtask build-app",
            ),
        ] {
            assert_eq!(recovery_text(step), Some(expected), "{step}");
        }
        assert!(
            recovery_text("use_bundled_cli")
                .is_some_and(|text| text.contains("~/.local/bin/stackly"))
        );
        assert!(recovery_text("check_again").is_some());
        assert_eq!(recovery_text("none"), None);
    }
}
