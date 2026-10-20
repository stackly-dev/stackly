//! The background agent port (spec 0015).
//!
//! [`BackgroundAgent`] reads and changes the OS registration of the daemon's
//! LaunchAgent. Only `stackly-cli` calls it, and only from explicit user
//! actions (`daemon install`, `daemon start`); `stacklyd` installs it so its
//! capability report stays honest and never calls it.
//!
//! [`LaunchContext::detect`] tells the daemon whether launchd started it.
//!
//! The mapping from an OS answer to a product state (`classify` in spec
//! 0015) is held back until the spec reflects the step 0 spike.

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
        /// The bundle the OS used (the process's main bundle).
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
    /// Not registered.
    NotRegistered,
    /// The OS could not find the service for this bundle.
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
    }
}
