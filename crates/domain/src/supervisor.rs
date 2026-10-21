//! The failure codes and stage names of the process supervisor (spec 0010,
//! *Kode error* and *Startup transaction*), next to the ones the operation
//! engine defines.

use crate::value::{FailureCode, StageName};

impl FailureCode {
    /// The main process exited on its own; the exit code goes in the
    /// details.
    pub const PROCESS_EXITED: Self = Self::from_static("process_exited");
    /// A signal ended the main process; the signal goes in the details.
    pub const PROCESS_SIGNALED: Self = Self::from_static("process_signaled");
    /// An adopted process exited, and the OS reports no status for a
    /// process that is not the daemon's child.
    pub const EXIT_STATUS_UNKNOWN: Self = Self::from_static("exit_status_unknown");
    /// The process crashed more often than the restart budget allows.
    pub const CRASH_LOOP: Self = Self::from_static("crash_loop");
    /// The OS refused to spawn the process.
    pub const SPAWN_FAILED: Self = Self::from_static("spawn_failed");
    /// The executable is not a canonical regular file inside its runtime
    /// root.
    pub const EXECUTABLE_INVALID: Self = Self::from_static("executable_invalid");
    /// The identity of the suspended child could not be captured or
    /// committed, so the child was discarded before it ran.
    pub const IDENTITY_CAPTURE_FAILED: Self = Self::from_static("identity_capture_failed");
    /// The configuration could not be rendered before the spawn.
    pub const CONFIG_RENDER_FAILED: Self = Self::from_static("config_render_failed");
    /// Another process took the endpoint's port.
    pub const ENDPOINT_IN_USE: Self = Self::from_static("endpoint_in_use");
    /// Another process holds the instance lock, or the last observed
    /// process still runs.
    pub const DATA_IN_USE: Self = Self::from_static("data_in_use");
    /// A `Run` step of the init plan exited with a code it does not accept.
    pub const INIT_STEP_FAILED: Self = Self::from_static("init_step_failed");
    /// A `Run` step of the init plan ran past its timeout.
    pub const INIT_STEP_TIMEOUT: Self = Self::from_static("init_step_timeout");
    /// The init plan ran past its deadline.
    pub const INIT_TIMEOUT: Self = Self::from_static("init_timeout");
    /// A runtime executable carries the quarantine attribute, so the start
    /// is refused before the spawn (spec 0018, K7). Stackly never removes
    /// quarantine.
    pub const QUARANTINED: Self = Self::from_static("quarantined");
    /// macOS killed the main process with `SIGKILL`, which the daemon did
    /// not send, before it was ready, for example for a broken signature
    /// (spec 0018, K7).
    pub const BINARY_REJECTED_BY_OS: Self = Self::from_static("binary_rejected_by_os");
}

impl StageName {
    /// Checks the instance can start, or be created.
    pub const VALIDATE: Self = Self::from_static("validate");
    /// Takes the operation's resource lock and the instance lock.
    pub const LOCK: Self = Self::from_static("lock");
    /// Checks the runtime, the package, the data folder, the secrets, and
    /// a dry run of the plan, before any side effect.
    pub const VERIFY: Self = Self::from_static("verify");
    /// Checks the fixed ports are free and picks the auto ports.
    pub const ENDPOINTS: Self = Self::from_static("endpoints");
    /// Commits the desired state and the lifecycle event that begins the
    /// action.
    pub const JOURNAL: Self = Self::from_static("journal");
    /// Builds the final plan and renders the configuration.
    pub const RENDER: Self = Self::from_static("render");
    /// Spawns the main process suspended.
    pub const SPAWN: Self = Self::from_static("spawn");
    /// Records the identity of the suspended child, then resumes it.
    pub const IDENTITY: Self = Self::from_static("identity");
    /// Probes readiness until ready, a crash, a cancel, or the deadline.
    pub const READINESS: Self = Self::from_static("readiness");
    /// Publishes the ready instance and its final ports.
    pub const PUBLISH: Self = Self::from_static("publish");
    /// Ends the operation and releases its lock.
    pub const COMPLETE: Self = Self::from_static("complete");
    /// Runs the provider's stop strategy and waits for the exit.
    pub const STOP: Self = Self::from_static("stop");
    /// Pins the provider package a new instance runs with.
    pub const PIN: Self = Self::from_static("pin");
    /// Stores the secrets of a new instance.
    pub const SECRETS: Self = Self::from_static("secrets");
    /// Creates the folders of a new instance.
    pub const LAYOUT: Self = Self::from_static("layout");
    /// Inserts a new instance and its first config revision.
    pub const REGISTER: Self = Self::from_static("register");
    /// Initializes the data of a new instance.
    pub const INIT: Self = Self::from_static("init");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_failure_codes_are_the_codes_of_spec_0010() {
        let codes = [
            FailureCode::INTERRUPTED,
            FailureCode::PROCESS_EXITED,
            FailureCode::PROCESS_SIGNALED,
            FailureCode::EXIT_STATUS_UNKNOWN,
            FailureCode::CRASH_LOOP,
            FailureCode::SPAWN_FAILED,
            FailureCode::EXECUTABLE_INVALID,
            FailureCode::IDENTITY_CAPTURE_FAILED,
            FailureCode::CONFIG_RENDER_FAILED,
            FailureCode::ENDPOINT_IN_USE,
            FailureCode::DATA_IN_USE,
            FailureCode::INIT_STEP_FAILED,
            FailureCode::INIT_STEP_TIMEOUT,
            FailureCode::INIT_TIMEOUT,
            FailureCode::QUARANTINED,
            FailureCode::BINARY_REJECTED_BY_OS,
        ];

        let text: Vec<_> = codes.iter().map(FailureCode::as_str).collect();
        assert_eq!(
            text,
            [
                "interrupted",
                "process_exited",
                "process_signaled",
                "exit_status_unknown",
                "crash_loop",
                "spawn_failed",
                "executable_invalid",
                "identity_capture_failed",
                "config_render_failed",
                "endpoint_in_use",
                "data_in_use",
                "init_step_failed",
                "init_step_timeout",
                "init_timeout",
                "quarantined",
                "binary_rejected_by_os",
            ]
        );
        for code in codes {
            assert_eq!(code.as_str().parse::<FailureCode>().unwrap(), code);
        }
    }

    #[test]
    fn startup_transaction_stages_follow_the_table_order() {
        let stages = [
            StageName::VALIDATE,
            StageName::LOCK,
            StageName::VERIFY,
            StageName::ENDPOINTS,
            StageName::JOURNAL,
            StageName::RENDER,
            StageName::SPAWN,
            StageName::IDENTITY,
            StageName::READINESS,
            StageName::PUBLISH,
            StageName::COMPLETE,
        ];

        let text: Vec<_> = stages.iter().map(StageName::as_str).collect();
        assert_eq!(
            text,
            [
                "validate",
                "lock",
                "verify",
                "endpoints",
                "journal",
                "render",
                "spawn",
                "identity",
                "readiness",
                "publish",
                "complete",
            ]
        );
        let others: Vec<_> = [
            StageName::STOP,
            StageName::PIN,
            StageName::SECRETS,
            StageName::LAYOUT,
            StageName::REGISTER,
            StageName::INIT,
        ]
        .iter()
        .map(|stage| stage.as_str().to_owned())
        .collect();
        assert_eq!(
            others,
            ["stop", "pin", "secrets", "layout", "register", "init"]
        );
    }
}
