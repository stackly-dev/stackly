//! The start order of `stacklyd` (spec 0006, *Urutan start*).
//!
//! Steps 1 to 8 run here: read the configuration, start the log writer,
//! resolve the roots (`--print-roots` stops there), prepare the layout,
//! create the server instance ID, take the daemon lock, install the log
//! file, and log the start line. The registry opens next (spec 0005), then
//! the daemon waits for SIGTERM or SIGINT. Milestone 4 of spec 0006 binds
//! the socket and serves clients between the start line and the wait, and
//! moves the registry into the start slot.

use std::ffi::{OsStr, OsString};
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

/// The variable that holds the `tracing` filter, for example `info` or
/// `stacklyd=debug`. Unset or empty means `info`.
pub const LOG_FILTER_ENV: &str = "STACKLY_LOG";
/// Exit code when the daemon cannot start.
pub const EXIT_START_FAILED: u8 = 1;
/// Exit code when another daemon holds the lock for the same data root.
pub const EXIT_LOCK_HELD: u8 = 3;

/// What `main` read from the arguments and the environment, once.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StartConfig {
    /// The value of `STACKLY_ROOT`, or `None` when it is unset.
    pub root_override: Option<OsString>,
    /// The value of `STACKLY_LOG`, or `None` when it is unset.
    pub log_filter: Option<OsString>,
    /// Whether `--print-roots` was given.
    pub print_roots: bool,
}

impl StartConfig {
    /// Bundles what `main` read.
    #[must_use]
    pub const fn new(
        root_override: Option<OsString>,
        log_filter: Option<OsString>,
        print_roots: bool,
    ) -> Self {
        Self {
            root_override,
            log_filter,
            print_roots,
        }
    }
}

/// The `STACKLY_LOG` value is not a valid `tracing` filter.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{LOG_FILTER_ENV} {value:?} is not a valid log filter: {reason}")]
#[non_exhaustive]
pub struct InvalidLogFilter {
    /// The value as read, converted lossily for display.
    pub value: String,
    /// Why the filter parser refused it.
    pub reason: String,
}

/// Parses `STACKLY_LOG`. Unset or empty means `info`.
///
/// # Errors
///
/// Returns [`InvalidLogFilter`] when the value is not UTF-8 or not a valid
/// `tracing` filter.
pub fn parse_log_filter(value: Option<&OsStr>) -> Result<EnvFilter, InvalidLogFilter> {
    let builder = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into());
    let Some(value) = value else {
        return Ok(builder.parse_lossy(""));
    };
    let invalid = |reason: String| InvalidLogFilter {
        value: value.to_string_lossy().into_owned(),
        reason,
    };
    let text = value
        .to_str()
        .ok_or_else(|| invalid("not valid UTF-8".to_owned()))?;
    builder
        .parse(text)
        .map_err(|error| invalid(error.to_string()))
}

/// Runs the daemon until SIGTERM or SIGINT and returns its exit code: 0
/// after a normal stop or `--print-roots`, [`EXIT_START_FAILED`] when it
/// cannot start, and [`EXIT_LOCK_HELD`] when another daemon holds the lock.
/// It installs the global `tracing` subscriber, so call it once per
/// process.
#[must_use]
pub fn run(config: StartConfig) -> ExitCode {
    imp::run(config)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::io;
    use std::process::ExitCode;

    use stackly_core::{daemon_info, prepare_layout};
    use stackly_platform::{DirectoryLayout, Platform, UserId};
    use stackly_platform_macos::MacosPlatform;
    use stackly_protocol::{CONTRACT_DIGEST, ProtocolVersion, Timestamp};
    use stackly_storage::{RegistryOpen, open_registry};
    use tokio::signal::unix::{Signal, SignalKind, signal};
    use tracing_subscriber::EnvFilter;
    use uuid::Uuid;

    use super::{EXIT_LOCK_HELD, EXIT_START_FAILED, StartConfig, parse_log_filter};
    use crate::lock::{DaemonLock, LockError, LockOwner};
    use crate::log_sink::{LOG_CONTROL_TIMEOUT, LogSink, LogSinkConfig};

    pub(super) fn run(config: StartConfig) -> ExitCode {
        // Step 1: the filter is checked before anything else, but reported
        // once the writer runs, so no line bypasses it.
        let filter = parse_log_filter(config.log_filter.as_deref());
        // Step 2: the log writer, stderr only for now.
        let sink = match LogSink::start(Box::new(io::stderr()), LogSinkConfig::DAEMON) {
            Ok(sink) => sink,
            Err(error) => {
                // The one direct write: there is no writer thread to use.
                eprintln!("stacklyd cannot start its log writer: {error}");
                return ExitCode::from(EXIT_START_FAILED);
            }
        };
        let (active, invalid) = match filter {
            Ok(filter) => (filter, None),
            Err(invalid) => (EnvFilter::new("info"), Some(invalid)),
        };
        let installed = tracing_subscriber::fmt()
            .json()
            .with_env_filter(active)
            .with_writer(sink.make_writer())
            .try_init();
        let code = match (installed, invalid) {
            (Err(error), _) => {
                // A subscriber is already set, so this line may go nowhere.
                tracing::error!(error = %error, "cannot install the log subscriber");
                EXIT_START_FAILED
            }
            (Ok(()), Some(invalid)) => {
                tracing::error!(error = %invalid, "cannot start stacklyd");
                EXIT_START_FAILED
            }
            (Ok(()), None) => start(&sink, config),
        };
        // Shutdown step 7: the queue gets at most a second to drain.
        let _ = sink.flush(LOG_CONTROL_TIMEOUT);
        ExitCode::from(code)
    }

    /// Steps 3 to 8, then the registry, then the wait for a signal.
    fn start(sink: &LogSink, config: StartConfig) -> u8 {
        let info = daemon_info();
        let has_override = config.root_override.is_some();

        // Step 3: resolve the roots once.
        let resolved = MacosPlatform::new(config.root_override).and_then(|platform| {
            platform
                .directories()
                .roots()
                .map(|roots| (platform, DirectoryLayout::new(roots)))
        });
        let (platform, layout) = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                tracing::error!(
                    error = %error,
                    root_override = has_override,
                    "cannot resolve the Stackly directories",
                );
                return EXIT_START_FAILED;
            }
        };
        if config.print_roots {
            let roots = layout.roots();
            tracing::info!(
                data_root = %roots.data().display(),
                logs_root = %roots.logs().display(),
                cache_root = %roots.cache().display(),
                runtime_root = %roots.runtime().display(),
                socket_path = %layout.daemon_socket().display(),
                root_override = has_override,
                "stacklyd resolved its directories",
            );
            return 0;
        }

        // Step 4: on an existing layout this changes nothing.
        let prepared = match prepare_layout(&platform) {
            Ok(prepared) => prepared,
            Err(error) => {
                tracing::error!(error = %error, "cannot prepare the Stackly directories");
                return EXIT_START_FAILED;
            }
        };
        let layout = prepared.layout();

        // Step 5.
        let server_instance_id = Uuid::now_v7();
        let started_at = Timestamp::from_jiff(platform.clock().now());
        let pid = std::process::id();

        // Step 6: nothing below runs without the lock.
        let owner = LockOwner::new(pid, server_instance_id, started_at, info.version);
        let lock = match DaemonLock::acquire(&layout.daemon_lock(), owner) {
            Ok(lock) => lock,
            Err(error) => return lock_failed(&error),
        };

        // Step 7.
        if sink.install_file(&layout.daemon_log_dir()).is_err() {
            tracing::warn!("the log writer did not install the log file in time");
        }

        // Step 8.
        let roots = layout.roots();
        tracing::info!(
            version = info.version,
            protocol_major = ProtocolVersion::CURRENT.major,
            protocol_minor = ProtocolVersion::CURRENT.minor,
            contract_digest = CONTRACT_DIGEST,
            server_instance_id = %server_instance_id,
            pid,
            uid = unix_uid(&platform),
            started_at = %started_at,
            data_root = %roots.data().display(),
            logs_root = %roots.logs().display(),
            cache_root = %roots.cache().display(),
            runtime_root = %roots.runtime().display(),
            socket_path = %layout.daemon_socket().display(),
            root_override = has_override,
            "stacklyd started",
        );

        // Signals are caught from here on, so a stop request while the
        // registry opens still stops the daemon cleanly.
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("stacklyd-rt")
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::error!(error = %error, "cannot start the async runtime");
                release(lock);
                return EXIT_START_FAILED;
            }
        };
        let signals = {
            let _context = runtime.enter();
            StopSignals::install()
        };
        let mut signals = match signals {
            Ok(signals) => signals,
            Err(error) => {
                tracing::error!(error = %error, "cannot install the signal handlers");
                release(lock);
                return EXIT_START_FAILED;
            }
        };

        let Some(registry) = open_registry_slot(&platform, layout, info.version) else {
            release(lock);
            return EXIT_START_FAILED;
        };

        let signal = runtime.block_on(signals.recv());
        tracing::info!(signal, "stacklyd stopping");
        // Joins the registry thread, which checkpoints and closes the file.
        drop(registry);
        release(lock);
        tracing::info!("stacklyd stopped");
        0
    }

    /// Opens the registry (spec 0005). `Some(None)` is recovery: the
    /// registry stays untouched and the daemon stays up (spec 0006 AC-18
    /// adds the recovery mode replies in milestone 4). `None` means the
    /// daemon must stop.
    fn open_registry_slot(
        platform: &Platform,
        layout: &DirectoryLayout,
        version: &str,
    ) -> Option<Option<stackly_storage::SqliteRegistry>> {
        match open_registry(platform, layout, version) {
            Ok(RegistryOpen::Ready { registry, summary }) => {
                tracing::info!(
                    registry_state = "ready",
                    schema_version = summary.schema_version,
                    created = summary.created,
                    migrated_from = summary.migrated_from,
                    snapshot = summary
                        .snapshot
                        .as_ref()
                        .map(|path| path.display().to_string()),
                    sqlite_version = summary.sqlite_version,
                    "registry ready",
                );
                Some(Some(registry))
            }
            Ok(RegistryOpen::Recovery(report)) => {
                tracing::error!(
                    registry_state = "recovery",
                    reason = report.reason.code(),
                    report = %report.path.display(),
                    "the registry needs recovery; it was left untouched and stacklyd stays up",
                );
                Some(None)
            }
            Err(error) => {
                tracing::error!(error = %error, "cannot open the registry");
                None
            }
        }
    }

    /// Logs why the lock was not taken and returns the exit code.
    fn lock_failed(error: &LockError) -> u8 {
        let Some(body) = error.to_error_body() else {
            tracing::error!(error = %error, "cannot take the daemon lock");
            return EXIT_START_FAILED;
        };
        let lock = match error {
            LockError::Held { path, .. } => path.display().to_string(),
            _ => String::new(),
        };
        let lock = lock.as_str();
        tracing::error!(
            code = body.code(),
            owner_pid = body.detail("pid"),
            owner_server_instance_id = body.detail("serverInstanceId"),
            owner_version = body.detail("version"),
            owner_started_at = body.detail("startedAt"),
            lock,
            "{}",
            body.message(),
        );
        EXIT_LOCK_HELD
    }

    fn release(lock: DaemonLock) {
        if let Err(error) = lock.release() {
            tracing::warn!(error = %error, "cannot release the daemon lock");
        }
    }

    fn unix_uid(platform: &Platform) -> Option<u32> {
        match platform.identity().current_user() {
            Ok(UserId::Unix(uid)) => Some(uid),
            _ => None,
        }
    }

    /// The two signals that stop the daemon.
    struct StopSignals {
        term: Signal,
        interrupt: Signal,
    }

    impl StopSignals {
        /// Installs the handlers; needs a runtime context.
        fn install() -> io::Result<Self> {
            Ok(Self {
                term: signal(SignalKind::terminate())?,
                interrupt: signal(SignalKind::interrupt())?,
            })
        }

        /// Waits for the first of the two and returns its name.
        async fn recv(&mut self) -> &'static str {
            tokio::select! {
                _ = self.term.recv() => "SIGTERM",
                _ = self.interrupt.recv() => "SIGINT",
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::process::ExitCode;

    use super::{EXIT_START_FAILED, StartConfig};

    pub(super) fn run(_config: StartConfig) -> ExitCode {
        tracing_subscriber::fmt().json().init();
        tracing::error!(
            version = stackly_core::daemon_info().version,
            "stacklyd only runs on macOS for now"
        );
        ExitCode::from(EXIT_START_FAILED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_or_empty_filter_means_info() {
        for value in [None, Some(OsStr::new(""))] {
            let filter = parse_log_filter(value).unwrap();

            assert_eq!(
                filter.max_level_hint(),
                Some(LevelFilter::INFO),
                "{value:?}"
            );
        }
    }

    #[test]
    fn a_valid_filter_is_kept() {
        let filter = parse_log_filter(Some(OsStr::new("stacklyd=debug,warn"))).unwrap();

        assert_eq!(filter.max_level_hint(), Some(LevelFilter::DEBUG));
    }

    #[test]
    fn an_invalid_filter_is_refused_with_its_value() {
        let error = parse_log_filter(Some(OsStr::new("stacklyd=loud"))).unwrap_err();

        assert_eq!(error.value, "stacklyd=loud");
        assert!(
            error
                .to_string()
                .starts_with(r#"STACKLY_LOG "stacklyd=loud" is not a valid log filter: "#),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_filter_that_is_not_utf8_is_refused() {
        use std::os::unix::ffi::OsStrExt;

        let error = parse_log_filter(Some(OsStr::from_bytes(b"info\xff"))).unwrap_err();

        assert_eq!(error.reason, "not valid UTF-8");
    }
}
