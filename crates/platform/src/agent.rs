//! The background agent port and the agent state model (spec 0015).
//!
//! [`BackgroundAgent`] reads and changes the OS registration of the daemon's
//! LaunchAgent. Only `stackly-cli` calls it, and only from explicit user
//! actions (`daemon install`, `daemon start`); `stacklyd` installs it so its
//! capability report stays honest and never calls it.
//!
//! [`classify`] turns what a client knows (the OS registration, a root
//! override, the launch record) into one of six product states with a
//! recovery step. The Swift app has the same table, and both are checked
//! against one shared fixture.
//!
//! [`LaunchContext::detect`] tells the daemon whether launchd started it.

use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;

use stackly_domain::Os;

use crate::bundle::{DAEMON_LABEL, TEST_AGENT_LABEL_PREFIX};
use crate::capability::PlatformCapability;

/// Reads and changes the registration of the daemon's LaunchAgent.
///
/// Every method is sync and may block on the OS service that keeps
/// background items, so async callers run it through `spawn_blocking` with
/// a time limit.
pub trait BackgroundAgent: Send + Sync {
    /// Reads the registration from the OS. Never prompts and never changes
    /// it.
    ///
    /// # Errors
    ///
    /// Returns [`BackgroundAgentError::NotSupported`] without an adapter and
    /// [`BackgroundAgentError::Os`] when the OS call fails.
    fn status(&self) -> Result<AgentProbe, BackgroundAgentError>;

    /// Registers the agent. The result may need the user's approval in
    /// System Settings.
    ///
    /// # Errors
    ///
    /// Returns [`BackgroundAgentError::NotSupported`] without an adapter and
    /// [`BackgroundAgentError::Os`] when the OS refuses.
    fn register(&self) -> Result<AgentProbe, BackgroundAgentError>;

    /// Unregisters the agent, so launchd stops launching it.
    ///
    /// # Errors
    ///
    /// Returns [`BackgroundAgentError::NotSupported`] without an adapter and
    /// [`BackgroundAgentError::Os`] when the OS refuses.
    fn unregister(&self) -> Result<AgentProbe, BackgroundAgentError>;

    /// Asks the OS to start the registered job now. On macOS this
    /// unregisters, then registers again, so `RunAtLoad` starts it; the one
    /// way to start, so the mechanism can change in the adapter alone.
    ///
    /// # Errors
    ///
    /// Returns [`BackgroundAgentError::NotSupported`] without an adapter and
    /// [`BackgroundAgentError::Os`] when the OS refuses either step.
    fn relaunch(&self) -> Result<AgentProbe, BackgroundAgentError>;
}

/// What the OS reported about the agent, or why it could not be asked.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AgentProbe {
    /// The OS answered for the bundle at `bundle`.
    Registration {
        /// The registration state the OS reported.
        registration: AgentRegistration,
        /// The canonical bundle the adapter was given.
        bundle: PathBuf,
    },
    /// The process does not run from inside the Stackly app bundle, so the
    /// OS cannot be asked.
    NotBundled {
        /// The executable path the process runs from.
        executable: PathBuf,
    },
    /// The process runs from a Stackly bundle that lacks the LaunchAgent
    /// property list or `stacklyd`, such as a build from the Xcode IDE.
    HelperMissing {
        /// The bundle folder.
        bundle: PathBuf,
    },
}

/// A registration state of the agent as the OS reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentRegistration {
    /// Registered and allowed to run.
    Enabled,
    /// Registered, but the user has not allowed it (or switched it off) in
    /// System Settings.
    RequiresApproval,
    /// Unregistered by the user's action (`unregister`); a bundle that was
    /// never registered reads [`AgentRegistration::NotFound`] instead.
    NotRegistered,
    /// The OS has no service for this bundle's identity: the answer for a
    /// bundle that was never registered, and after a registration the OS
    /// accepted but still cannot find.
    NotFound,
    /// A status value this build does not know.
    Unrecognized(i64),
}

impl AgentRegistration {
    /// Returns the stable snake_case code, for example `requires_approval`.
    /// An unknown OS value is `unrecognized`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::RequiresApproval => "requires_approval",
            Self::NotRegistered => "not_registered",
            Self::NotFound => "not_found",
            Self::Unrecognized(_) => "unrecognized",
        }
    }
}

/// A call on the [`BackgroundAgent`] port, as named in errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentOp {
    /// [`BackgroundAgent::status`].
    Status,
    /// [`BackgroundAgent::register`].
    Register,
    /// [`BackgroundAgent::unregister`].
    Unregister,
    /// [`BackgroundAgent::relaunch`].
    Relaunch,
}

impl AgentOp {
    /// Returns the stable snake_case code, for example `register`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Register => "register",
            Self::Unregister => "unregister",
            Self::Relaunch => "relaunch",
        }
    }
}

impl fmt::Display for AgentOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// The kind of an OS error, from the table *Hasil pendaftaran* of spec 0015.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentOsErrorKind {
    /// The agent was already registered; callers read the status again.
    AlreadyRegistered,
    /// The user denied the launch in System Settings.
    LaunchDenied,
    /// The job must be enabled by the user first.
    MustBeEnabled,
    /// The OS rejected the bundle's code signature.
    InvalidSignature,
    /// The OS could not find the job or its property list.
    NotFound,
    /// Any other error.
    Other,
}

impl AgentOsErrorKind {
    /// Returns the stable snake_case code, for example `launch_denied`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::AlreadyRegistered => "already_registered",
            Self::LaunchDenied => "launch_denied",
            Self::MustBeEnabled => "must_be_enabled",
            Self::InvalidSignature => "invalid_signature",
            Self::NotFound => "not_found",
            Self::Other => "other",
        }
    }
}

/// A failure of the [`BackgroundAgent`] port.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BackgroundAgentError {
    /// The OS has no background agent adapter.
    #[error("the {capability} capability is not supported on {os}")]
    NotSupported {
        /// Always [`PlatformCapability::BackgroundAgent`].
        capability: PlatformCapability,
        /// The OS that lacks it.
        os: Os,
    },
    /// The OS call failed.
    #[error("{op} failed with {domain} {code}: {message}")]
    Os {
        /// The call that failed.
        op: AgentOp,
        /// The kind of the error.
        kind: AgentOsErrorKind,
        /// The OS error domain, for example `SMAppServiceErrorDomain`.
        domain: String,
        /// The OS error code.
        code: i64,
        /// The OS error description, safe to show (it names no secret).
        message: String,
    },
}

/// The port of an OS without a background agent adapter: every method
/// answers [`BackgroundAgentError::NotSupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedBackgroundAgent {
    os: Os,
}

impl UnsupportedBackgroundAgent {
    /// Creates the port for `os`.
    #[must_use]
    pub const fn new(os: Os) -> Self {
        Self { os }
    }

    const fn not_supported(&self) -> BackgroundAgentError {
        BackgroundAgentError::NotSupported {
            capability: PlatformCapability::BackgroundAgent,
            os: self.os,
        }
    }
}

impl BackgroundAgent for UnsupportedBackgroundAgent {
    fn status(&self) -> Result<AgentProbe, BackgroundAgentError> {
        Err(self.not_supported())
    }

    fn register(&self) -> Result<AgentProbe, BackgroundAgentError> {
        Err(self.not_supported())
    }

    fn unregister(&self) -> Result<AgentProbe, BackgroundAgentError> {
        Err(self.not_supported())
    }

    fn relaunch(&self) -> Result<AgentProbe, BackgroundAgentError> {
        Err(self.not_supported())
    }
}

/// The outcome of asking for the agent's status, as [`classify`] takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The OS was not asked, which happens only with a root override.
    NotChecked,
    /// The OS has no background agent adapter.
    Unsupported,
    /// The status call failed: with the OS code, or `None` for a time out.
    Failed {
        /// The OS error code, or `None` when the call timed out.
        code: Option<i64>,
    },
    /// The OS answered.
    Probe(AgentProbe),
}

/// Everything [`classify`] needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInput {
    /// Whether `STACKLY_ROOT` is set; the agent only ever serves the default
    /// root.
    pub root_override: bool,
    /// What the OS reported.
    pub probe: ProbeOutcome,
    /// Whether the launch record of the default root exists, which proves
    /// the agent ran under launchd before.
    pub launch_record: bool,
    /// Whether the probe was read inside one registration action, right after
    /// `register` or `relaunch` succeeded, failed with `AlreadyRegistered`,
    /// or failed with [`AgentOsErrorKind::NotFound`]. Only then does a
    /// `NotFound` mean the OS cannot find the service; otherwise the bundle
    /// was never registered.
    pub just_registered: bool,
}

/// One of the six product states of the background agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackgroundAgentState {
    /// Registered and allowed to run.
    Enabled,
    /// Waiting for the user's approval in Login Items.
    RequiresApproval,
    /// Switched off in Login Items after it ran before.
    Disabled,
    /// Not registered, including a bundle that was never registered.
    NotRegistered,
    /// The OS accepted a registration in the same action but still cannot
    /// find the service for this copy of the app.
    NotFound,
    /// The state cannot be known here; [`UnavailableReason`] says why.
    Unavailable,
}

impl BackgroundAgentState {
    /// Returns the stable snake_case code, for example `requires_approval`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::RequiresApproval => "requires_approval",
            Self::Disabled => "disabled",
            Self::NotRegistered => "not_registered",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Why the state is [`BackgroundAgentState::Unavailable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnavailableReason {
    /// `STACKLY_ROOT` is set.
    RootOverride,
    /// The OS has no background agent adapter.
    UnsupportedOs,
    /// The status call failed or timed out.
    StatusFailed,
    /// The process does not run from inside the Stackly app bundle.
    NotBundled,
    /// The bundle lacks the LaunchAgent property list or `stacklyd`.
    HelperMissing,
    /// The OS reported a status value this build does not know.
    StatusUnrecognized,
}

impl UnavailableReason {
    /// Returns the stable snake_case code, for example `root_override`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::RootOverride => "root_override",
            Self::UnsupportedOs => "unsupported_os",
            Self::StatusFailed => "status_failed",
            Self::NotBundled => "not_bundled",
            Self::HelperMissing => "helper_missing",
            Self::StatusUnrecognized => "status_unrecognized",
        }
    }
}

/// The next step a client offers for a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryStep {
    /// Nothing to do.
    None,
    /// Start the daemon; [`classify`] never returns it, a caller that knows
    /// the daemon is unreachable uses it for [`BackgroundAgentState::Enabled`].
    Start,
    /// Register the agent.
    Register,
    /// Allow Stackly in System Settings > General > Login Items & Extensions.
    ApproveInLoginItems,
    /// Turn Stackly on again in Login Items.
    EnableInLoginItems,
    /// Move the app to the Applications folder or install it again.
    MoveOrReinstallApp,
    /// Use the `stackly` inside `Stackly.app`, or start `stacklyd` by hand.
    UseBundledCli,
    /// Start `stacklyd` by hand for the root override.
    ManualDaemonForOverride,
    /// Build the app with `cargo xtask build-app`.
    BuildWithXtask,
    /// Check again.
    CheckAgain,
}

impl RecoveryStep {
    /// Returns the stable snake_case code, for example
    /// `approve_in_login_items`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Start => "start",
            Self::Register => "register",
            Self::ApproveInLoginItems => "approve_in_login_items",
            Self::EnableInLoginItems => "enable_in_login_items",
            Self::MoveOrReinstallApp => "move_or_reinstall_app",
            Self::UseBundledCli => "use_bundled_cli",
            Self::ManualDaemonForOverride => "manual_daemon_for_override",
            Self::BuildWithXtask => "build_with_xtask",
            Self::CheckAgain => "check_again",
        }
    }
}

/// The result of [`classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentAssessment {
    /// The product state.
    pub state: BackgroundAgentState,
    /// Why the state is unavailable, set only for
    /// [`BackgroundAgentState::Unavailable`].
    pub reason: Option<UnavailableReason>,
    /// The next step to offer.
    pub recovery: RecoveryStep,
}

impl AgentAssessment {
    const fn of(state: BackgroundAgentState, recovery: RecoveryStep) -> Self {
        Self {
            state,
            reason: None,
            recovery,
        }
    }

    const fn unavailable(reason: UnavailableReason, recovery: RecoveryStep) -> Self {
        Self {
            state: BackgroundAgentState::Unavailable,
            reason: Some(reason),
            recovery,
        }
    }
}

/// Maps what a client knows to a product state, with the rows of the table
/// *State background agent* in spec 0015, evaluated from the top.
///
/// `classify` does not know whether the daemon answers; a caller that knows
/// it does not and gets [`BackgroundAgentState::Enabled`] offers
/// [`RecoveryStep::Start`].
#[must_use]
pub fn classify(input: &AgentInput) -> AgentAssessment {
    use AgentAssessment as A;
    use BackgroundAgentState as S;
    use RecoveryStep as R;
    use UnavailableReason as U;

    if input.root_override {
        return A::unavailable(U::RootOverride, R::ManualDaemonForOverride);
    }
    match &input.probe {
        ProbeOutcome::Unsupported => A::unavailable(U::UnsupportedOs, R::None),
        ProbeOutcome::Failed { .. } | ProbeOutcome::NotChecked => {
            A::unavailable(U::StatusFailed, R::CheckAgain)
        }
        ProbeOutcome::Probe(AgentProbe::NotBundled { .. }) => {
            A::unavailable(U::NotBundled, R::UseBundledCli)
        }
        ProbeOutcome::Probe(AgentProbe::HelperMissing { .. }) => {
            A::unavailable(U::HelperMissing, R::BuildWithXtask)
        }
        ProbeOutcome::Probe(AgentProbe::Registration { registration, .. }) => match registration {
            AgentRegistration::Unrecognized(_) => {
                A::unavailable(U::StatusUnrecognized, R::CheckAgain)
            }
            AgentRegistration::Enabled => A::of(S::Enabled, R::None),
            AgentRegistration::NotRegistered => A::of(S::NotRegistered, R::Register),
            AgentRegistration::NotFound => {
                if input.just_registered {
                    A::of(S::NotFound, R::MoveOrReinstallApp)
                } else {
                    A::of(S::NotRegistered, R::Register)
                }
            }
            AgentRegistration::RequiresApproval => {
                if !input.launch_record || input.just_registered {
                    A::of(S::RequiresApproval, R::ApproveInLoginItems)
                } else {
                    A::of(S::Disabled, R::EnableInLoginItems)
                }
            }
        },
    }
}

/// Who started the daemon process.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LaunchContext {
    /// launchd, as the job with `label`.
    Launchd {
        /// The job label, the production label or a test agent label.
        label: String,
    },
    /// Anything else: a terminal, a test, or a process that was orphaned to
    /// launchd without being its job.
    Manual,
}

impl LaunchContext {
    /// Detects the context from `XPC_SERVICE_NAME` and the parent PID, both
    /// read once in `main`.
    ///
    /// It is [`LaunchContext::Launchd`] only when the parent is launchd
    /// (PID 1) and `XPC_SERVICE_NAME` is the production label or starts with
    /// the test agent prefix: a process started from Terminal inherits
    /// Terminal's `XPC_SERVICE_NAME`, and an orphaned one has parent PID 1,
    /// so neither alone is enough.
    #[must_use]
    pub fn detect(xpc_service_name: Option<&OsStr>, parent_pid: u32) -> Self {
        if parent_pid != 1 {
            return Self::Manual;
        }
        match xpc_service_name.and_then(OsStr::to_str) {
            Some(label)
                if label == DAEMON_LABEL
                    || (label.starts_with(TEST_AGENT_LABEL_PREFIX)
                        && label.len() > TEST_AGENT_LABEL_PREFIX.len()) =>
            {
                Self::Launchd {
                    label: label.to_owned(),
                }
            }
            _ => Self::Manual,
        }
    }

    /// Returns the job label under launchd, or `None` for a manual start.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Launchd { label } => Some(label),
            Self::Manual => None,
        }
    }

    /// Returns whether launchd started the production agent, the only
    /// context that writes the launch record.
    #[must_use]
    pub fn is_production_agent(&self) -> bool {
        self.label() == Some(DAEMON_LABEL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered(registration: AgentRegistration) -> ProbeOutcome {
        ProbeOutcome::Probe(AgentProbe::Registration {
            registration,
            bundle: PathBuf::from("/Applications/Stackly.app"),
        })
    }

    fn input(probe: ProbeOutcome) -> AgentInput {
        AgentInput {
            root_override: false,
            probe,
            launch_record: false,
            just_registered: false,
        }
    }

    fn assess(input: &AgentInput) -> (&'static str, Option<&'static str>, &'static str) {
        let a = classify(input);
        (
            a.state.as_code(),
            a.reason.map(UnavailableReason::as_code),
            a.recovery.as_code(),
        )
    }

    #[test]
    fn a_root_override_wins_over_every_os_answer() {
        let mut i = input(registered(AgentRegistration::Enabled));
        i.root_override = true;
        assert_eq!(
            assess(&i),
            (
                "unavailable",
                Some("root_override"),
                "manual_daemon_for_override"
            )
        );
    }

    #[test]
    fn a_root_override_without_an_os_call_is_unavailable() {
        let mut i = input(ProbeOutcome::NotChecked);
        i.root_override = true;
        assert_eq!(classify(&i).reason, Some(UnavailableReason::RootOverride));
    }

    #[test]
    fn not_checked_without_a_root_override_counts_as_a_failed_status() {
        assert_eq!(
            assess(&input(ProbeOutcome::NotChecked)),
            ("unavailable", Some("status_failed"), "check_again")
        );
    }

    #[test]
    fn an_unsupported_os_has_no_recovery() {
        assert_eq!(
            assess(&input(ProbeOutcome::Unsupported)),
            ("unavailable", Some("unsupported_os"), "none")
        );
    }

    #[test]
    fn a_timed_out_status_asks_to_check_again() {
        assert_eq!(
            assess(&input(ProbeOutcome::Failed { code: None })),
            ("unavailable", Some("status_failed"), "check_again")
        );
    }

    #[test]
    fn a_cli_outside_the_bundle_points_to_the_bundled_cli() {
        let probe = ProbeOutcome::Probe(AgentProbe::NotBundled {
            executable: PathBuf::from("/work/target/debug/stackly"),
        });
        assert_eq!(
            assess(&input(probe)),
            ("unavailable", Some("not_bundled"), "use_bundled_cli")
        );
    }

    #[test]
    fn a_bundle_without_the_helper_points_to_build_app() {
        let probe = ProbeOutcome::Probe(AgentProbe::HelperMissing {
            bundle: PathBuf::from("/DerivedData/Stackly.app"),
        });
        assert_eq!(
            assess(&input(probe)),
            ("unavailable", Some("helper_missing"), "build_with_xtask")
        );
    }

    #[test]
    fn an_unknown_os_status_is_unavailable() {
        assert_eq!(
            assess(&input(registered(AgentRegistration::Unrecognized(9)))),
            ("unavailable", Some("status_unrecognized"), "check_again")
        );
    }

    #[test]
    fn enabled_has_no_recovery() {
        assert_eq!(
            assess(&input(registered(AgentRegistration::Enabled))),
            ("enabled", None, "none")
        );
    }

    #[test]
    fn not_registered_offers_to_register() {
        assert_eq!(
            assess(&input(registered(AgentRegistration::NotRegistered))),
            ("not_registered", None, "register")
        );
    }

    #[test]
    fn a_never_registered_bundle_offers_to_register() {
        for launch_record in [false, true] {
            let mut i = input(registered(AgentRegistration::NotFound));
            i.launch_record = launch_record;
            assert_eq!(assess(&i), ("not_registered", None, "register"));
        }
    }

    #[test]
    fn not_found_right_after_registering_offers_to_move_or_reinstall() {
        for launch_record in [false, true] {
            let mut i = input(registered(AgentRegistration::NotFound));
            i.launch_record = launch_record;
            i.just_registered = true;
            assert_eq!(assess(&i), ("not_found", None, "move_or_reinstall_app"));
        }
    }

    #[test]
    fn requires_approval_without_a_launch_record_waits_for_approval() {
        assert_eq!(
            assess(&input(registered(AgentRegistration::RequiresApproval))),
            ("requires_approval", None, "approve_in_login_items")
        );
    }

    #[test]
    fn requires_approval_with_a_launch_record_is_disabled() {
        let mut i = input(registered(AgentRegistration::RequiresApproval));
        i.launch_record = true;
        assert_eq!(assess(&i), ("disabled", None, "enable_in_login_items"));
    }

    #[test]
    fn a_registration_in_the_same_action_is_never_disabled() {
        let mut i = input(registered(AgentRegistration::RequiresApproval));
        i.launch_record = true;
        i.just_registered = true;
        assert_eq!(
            assess(&i),
            ("requires_approval", None, "approve_in_login_items")
        );
    }

    #[test]
    fn only_the_unavailable_state_carries_a_reason() {
        for registration in [
            AgentRegistration::Enabled,
            AgentRegistration::RequiresApproval,
            AgentRegistration::NotRegistered,
            AgentRegistration::NotFound,
        ] {
            assert_eq!(classify(&input(registered(registration))).reason, None);
        }
    }

    #[test]
    fn the_unsupported_port_answers_not_supported_for_its_os() {
        let port = UnsupportedBackgroundAgent::new(Os::Windows);
        let expected = BackgroundAgentError::NotSupported {
            capability: PlatformCapability::BackgroundAgent,
            os: Os::Windows,
        };
        assert_eq!(port.status(), Err(expected.clone()));
        assert_eq!(port.register(), Err(expected.clone()));
        assert_eq!(port.unregister(), Err(expected.clone()));
        assert_eq!(port.relaunch(), Err(expected));
    }

    #[test]
    fn errors_print_in_lowercase_without_a_period() {
        let error = BackgroundAgentError::Os {
            op: AgentOp::Register,
            kind: AgentOsErrorKind::Other,
            domain: "SMAppServiceErrorDomain".to_owned(),
            code: 2,
            message: "internal failure".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "register failed with SMAppServiceErrorDomain 2: internal failure"
        );
        let unsupported = UnsupportedBackgroundAgent::new(Os::Linux)
            .status()
            .expect_err("unsupported");
        assert_eq!(
            unsupported.to_string(),
            "the background_agent capability is not supported on linux"
        );
    }

    #[test]
    fn launchd_with_the_production_label_is_the_production_agent() {
        let context = LaunchContext::detect(Some(OsStr::new(DAEMON_LABEL)), 1);
        assert_eq!(context.label(), Some(DAEMON_LABEL));
        assert!(context.is_production_agent());
    }

    #[test]
    fn launchd_with_a_test_label_is_launchd_but_not_the_production_agent() {
        let context = LaunchContext::detect(Some(OsStr::new("dev.stackly.test.agent.42-1")), 1);
        assert_eq!(context.label(), Some("dev.stackly.test.agent.42-1"));
        assert!(!context.is_production_agent());
    }

    #[test]
    fn a_terminal_start_with_an_inherited_service_name_is_manual() {
        let context =
            LaunchContext::detect(Some(OsStr::new("application.com.apple.Terminal.1234")), 1);
        assert_eq!(context, LaunchContext::Manual);
    }

    #[test]
    fn the_production_label_with_another_parent_is_manual() {
        assert_eq!(
            LaunchContext::detect(Some(OsStr::new(DAEMON_LABEL)), 4242),
            LaunchContext::Manual
        );
    }

    #[test]
    fn no_service_name_is_manual() {
        assert_eq!(LaunchContext::detect(None, 1), LaunchContext::Manual);
    }

    #[test]
    fn the_bare_test_prefix_is_not_a_label() {
        assert_eq!(
            LaunchContext::detect(Some(OsStr::new(TEST_AGENT_LABEL_PREFIX)), 1),
            LaunchContext::Manual
        );
    }

    #[test]
    fn codes_are_stable_snake_case() {
        assert_eq!(AgentRegistration::Unrecognized(7).as_code(), "unrecognized");
        assert_eq!(AgentOp::Relaunch.as_code(), "relaunch");
        assert_eq!(
            AgentOsErrorKind::AlreadyRegistered.as_code(),
            "already_registered"
        );
        assert_eq!(BackgroundAgentState::Disabled.as_code(), "disabled");
        assert_eq!(RecoveryStep::Start.as_code(), "start");
    }
}
