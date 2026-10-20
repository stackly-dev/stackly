//! The catalog codes the supervisor fails an operation with (spec 0010,
//! *Kode error*). `stacklyd` looks each one up in the catalog of
//! `stackly-protocol`; step 12 of spec 0010 adds the `supervisor` and
//! `process` areas there and a test that keeps this list and the catalog
//! in step.

use stackly_domain::ErrorCodeName;

macro_rules! codes {
    ($($(#[$doc:meta])* $name:ident = $code:literal;)*) => {
        $($(#[$doc])* pub const $name: ErrorCodeName = ErrorCodeName::from_static($code);)*

        /// Every code above, for the catalog test of `stackly-protocol`.
        pub const ALL: &[ErrorCodeName] = &[$($name),*];
    };
}

codes! {
    /// A start before `init_succeeded` (BR-008).
    NOT_INITIALIZED = "supervisor.not_initialized";
    /// A stop while the data is being initialized.
    INSTANCE_INITIALIZING = "supervisor.instance_initializing";
    /// A start while the instance is stopping.
    INSTANCE_STOPPING = "supervisor.instance_stopping";
    /// An action on a retained instance.
    INSTANCE_RETAINED = "supervisor.instance_retained";
    /// A start or stop on an `unknown` instance.
    IDENTITY_UNPROVEN = "supervisor.identity_unproven";
    /// The instance lock is held, or the last observed process still runs.
    DATA_IN_USE = "supervisor.data_in_use";
    /// Another process holds the endpoint's port.
    ENDPOINT_IN_USE = "supervisor.endpoint_in_use";
    /// The OS refused the trial bind of a `Fixed` port, or the address is
    /// not available.
    ENDPOINT_UNAVAILABLE = "supervisor.endpoint_unavailable";
    /// Another Stackly instance's effective port conflicts.
    ENDPOINT_CONFLICT = "supervisor.endpoint_conflict";
    /// No free candidate for an `Auto` port.
    AUTO_PORT_UNAVAILABLE = "supervisor.auto_port_unavailable";
    /// The runtime or one it depends on is not `installed`.
    RUNTIME_UNAVAILABLE = "supervisor.runtime_unavailable";
    /// The pinned package is missing, invalid, or has another digest.
    PACKAGE_UNAVAILABLE = "supervisor.package_unavailable";
    /// The dry run of the plan failed.
    CONFIG_INVALID = "supervisor.config_invalid";
    /// The executable is not a canonical regular file inside its runtime.
    EXECUTABLE_INVALID = "supervisor.executable_invalid";
    /// The instance folder is missing, not the user's, or not private.
    DATA_UNAVAILABLE = "supervisor.data_unavailable";
    /// `preflight` answered anything but `Compatible`.
    DATA_INCOMPATIBLE = "supervisor.data_incompatible";
    /// The readiness deadline passed; the process keeps running.
    READINESS_TIMEOUT = "supervisor.readiness_timeout";
    /// The restart budget ran out during a start.
    RETRY_BUDGET_EXHAUSTED = "supervisor.retry_budget_exhausted";
    /// The stop budget ran out.
    STOP_TIMEOUT = "supervisor.stop_timeout";
    /// An environment lease needs an override to stop the instance.
    STOP_REQUIRES_OVERRIDE = "supervisor.stop_requires_override";
    /// The stop command exited with a code it does not accept.
    STOP_COMMAND_FAILED = "supervisor.stop_command_failed";
    /// The reload action or its verification failed.
    RELOAD_FAILED = "supervisor.reload_failed";
    /// The provider lacks the capability (BR-016).
    CAPABILITY_UNSUPPORTED = "supervisor.capability_unsupported";
    /// The init plan ended `failed`.
    INIT_FAILED = "supervisor.init_failed";
    /// `posix_spawn` failed.
    SPAWN_FAILED = "process.spawn_failed";
    /// The identity of the suspended child could not be captured or
    /// recorded.
    IDENTITY_CAPTURE_FAILED = "process.identity_capture_failed";
    /// The identity changed at the check right before a signal.
    SIGNAL_REFUSED = "process.signal_refused";
    /// The platform has no process supervision.
    PROCESS_UNSUPPORTED = "process.unsupported";
}
