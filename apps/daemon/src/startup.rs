//! The start order of `stacklyd` (spec 0006, *Urutan start*).
//!
//! Steps 1 to 8 run here: read the configuration, start the log writer,
//! resolve the roots and check the catalog folder (`--print-roots` stops
//! there), prepare the layout,
//! create the server instance ID, take the daemon lock, install the log
//! file, and log the start line. [`crate::server`] then binds the socket,
//! serves, and runs the start slot (the registry, spec 0005) until SIGTERM,
//! SIGINT, or a fatal error, and runs the stop order; the lock is released
//! last.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use stackly_core::catalog::CatalogLocation;
use stackly_platform::{FileKind, Filesystem};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

/// The variable that holds the `tracing` filter, for example `info` or
/// `stacklyd=debug`. Unset or empty means `info`.
pub const LOG_FILTER_ENV: &str = "STACKLY_LOG";
/// The variable that names a catalog folder built by `cargo xtask catalog
/// build`, for development and tests (spec 0017). A catalog from it is
/// always labelled `development`, never `official`.
pub const CATALOG_DIR_ENV: &str = "STACKLY_CATALOG_DIR";
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
    /// The value of `STACKLY_CATALOG_DIR`, or `None` when it is unset.
    pub catalog_dir: Option<OsString>,
    /// The value of `XPC_SERVICE_NAME`, or `None` when it is unset.
    pub xpc_service_name: Option<OsString>,
    /// The parent process ID; 1 means launchd (spec 0015).
    pub parent_pid: u32,
    /// The path of the running executable as the OS reports it, or `None`
    /// when it cannot be read.
    pub current_exe: Option<PathBuf>,
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
            catalog_dir: None,
            xpc_service_name: None,
            parent_pid: 0,
            current_exe: None,
        }
    }

    /// Adds the value of `STACKLY_CATALOG_DIR`.
    #[must_use]
    pub fn with_catalog_dir(mut self, catalog_dir: Option<OsString>) -> Self {
        self.catalog_dir = catalog_dir;
        self
    }

    /// Adds what tells who started the daemon: `XPC_SERVICE_NAME`, the
    /// parent PID, and the path of the executable (spec 0015). `main` reads
    /// all three once.
    #[must_use]
    pub fn with_launch_environment(
        mut self,
        xpc_service_name: Option<OsString>,
        parent_pid: u32,
        current_exe: Option<PathBuf>,
    ) -> Self {
        self.xpc_service_name = xpc_service_name;
        self.parent_pid = parent_pid;
        self.current_exe = current_exe;
        self
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

/// The `STACKLY_CATALOG_DIR` value is not an absolute path to a folder.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{CATALOG_DIR_ENV} {value:?} is not an absolute path to a folder")]
#[non_exhaustive]
pub struct InvalidCatalogDir {
    /// The value as read, converted lossily for display.
    pub value: String,
}

/// Checks `STACKLY_CATALOG_DIR` through `fs`: unset means no catalog
/// folder; otherwise the value must be an absolute path to a folder, whose
/// canonical path becomes a development catalog location. It only reads.
///
/// # Errors
///
/// Returns [`InvalidCatalogDir`] for an empty or relative value, a path
/// that does not exist, and a path that is not a folder.
pub fn parse_catalog_dir(
    fs: &dyn Filesystem,
    value: Option<&OsStr>,
) -> Result<Option<CatalogLocation>, InvalidCatalogDir> {
    let Some(value) = value else {
        return Ok(None);
    };
    let invalid = || InvalidCatalogDir {
        value: value.to_string_lossy().into_owned(),
    };
    let path = Path::new(value);
    if value.is_empty() || !path.is_absolute() {
        return Err(invalid());
    }
    let canonical = fs.canonicalize(path).map_err(|_| invalid())?;
    match fs.symlink_metadata(&canonical) {
        Ok(Some(info)) if info.kind == FileKind::Dir => {
            Ok(Some(CatalogLocation::Development(canonical)))
        }
        _ => Err(invalid()),
    }
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
    use std::sync::Arc;
    use std::time::Duration;

    use stackly_core::secret::{Redactor, SecretWorker, WorkerConfig};
    use stackly_core::{daemon_info, prepare_layout};
    use stackly_platform::bundle::AppBundle;
    use stackly_platform::{DirectoryLayout, LaunchContext, Platform, SecretStore, UserId};
    use stackly_platform_macos::{KeychainTarget, MacosPlatform, MacosSecretStore};
    use stackly_protocol::events::ShutdownReason;
    use stackly_protocol::secret::SecretStoreTarget;
    use stackly_protocol::{CONTRACT_DIGEST, ProtocolVersion, Timestamp};
    use tracing_subscriber::EnvFilter;
    use uuid::Uuid;

    use super::{
        EXIT_LOCK_HELD, EXIT_START_FAILED, StartConfig, parse_catalog_dir, parse_log_filter,
    };
    use crate::lock::{DaemonLock, LockError, LockOwner};
    use crate::log_sink::{LOG_CONTROL_TIMEOUT, LogSink, LogSinkConfig, RedactingMakeWriter};
    use crate::map::StartFacts;
    use crate::server::{self, DaemonDeps};
    use crate::shutdown::StopSignals;

    /// How long blocking tasks get when the runtime shuts down.
    const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(1);

    pub(super) fn run(config: StartConfig) -> ExitCode {
        // Step 1: the filter is checked before anything else, but reported
        // once the writer runs, so no line bypasses it.
        let filter = parse_log_filter(config.log_filter.as_deref());
        // Step 2: the log writer, stderr only for now.
        let sink = match LogSink::start(Box::new(io::stderr()), LogSinkConfig::DAEMON) {
            Ok(sink) => Arc::new(sink),
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
        // The one redactor of the run: every log line passes it after it was
        // formatted, and the daemon hands the same one to the secret
        // service, the error bodies, and the events (spec 0011 AC-13).
        let redactor = Arc::new(Redactor::new());
        let installed = tracing_subscriber::fmt()
            .json()
            .with_env_filter(active)
            .with_writer(RedactingMakeWriter::new(
                sink.make_writer(),
                Arc::clone(&redactor),
            ))
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
            (Ok(()), None) => start(&sink, &redactor, config),
        };
        // Shutdown step 7: the queue gets at most a second to drain.
        let _ = sink.flush(LOG_CONTROL_TIMEOUT);
        ExitCode::from(code)
    }

    /// Steps 3 to 8, then the server until it stops.
    fn start(sink: &Arc<LogSink>, redactor: &Arc<Redactor>, config: StartConfig) -> u8 {
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
        // Who started the daemon, and the app bundle it runs from. The
        // executable path is made canonical so a symlink into the bundle
        // still finds it; a path that cannot be resolved is no bundle.
        let launch = LaunchContext::detect(config.xpc_service_name.as_deref(), config.parent_pid);
        let bundle = config
            .current_exe
            .as_deref()
            .and_then(|exe| platform.fs().canonicalize(exe).ok())
            .and_then(|exe| AppBundle::enclosing(&exe));
        // Step 3 also checks the catalog folder, which only reads.
        let catalog = match parse_catalog_dir(platform.fs(), config.catalog_dir.as_deref()) {
            Ok(catalog) => catalog,
            Err(error) => {
                tracing::error!(error = %error, "cannot start stacklyd");
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
            launch_context = if launch.label().is_some() { "launchd" } else { "manual" },
            launch_label = launch.label(),
            bundle_path = bundle.as_ref().map(|bundle| bundle.root().display().to_string()),
            "stacklyd started",
        );

        // The secret store: the login Keychain for the default root, a file
        // keychain under the data root for `STACKLY_ROOT` (spec 0011,
        // *Adapter Keychain macOS*). It is set up after the lock, since it
        // writes to the data root, and a store that cannot be set up stops
        // the daemon instead of running with one that is silently locked.
        let (keychain, secret_target) = if has_override {
            (
                KeychainTarget::in_dir(&layout.keychain_dir()),
                SecretStoreTarget::File,
            )
        } else {
            (KeychainTarget::Login, SecretStoreTarget::Login)
        };
        let store = match MacosSecretStore::new(keychain) {
            Ok(store) => store,
            Err(error) => {
                tracing::error!(error = %error, "cannot set up the secret store");
                release(lock);
                return EXIT_START_FAILED;
            }
        };
        let worker = match SecretWorker::spawn(Arc::new(store), WorkerConfig::default()) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::error!(error = %error, "cannot start the secret store thread");
                release(lock);
                return EXIT_START_FAILED;
            }
        };
        let platform = platform.with_secrets(Arc::clone(&worker) as Arc<dyn SecretStore>);

        // Signals are caught from here on, so a stop request while the
        // socket binds or the registry opens still stops the daemon cleanly.
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

        let facts = StartFacts::new(server_instance_id, started_at, has_override, pid);
        let deps = DaemonDeps::new(platform, prepared, facts, Arc::clone(sink))
            .with_launch(launch, bundle)
            .with_catalog_location(catalog)
            .with_redactor(Arc::clone(redactor))
            .with_secret_target(Some(secret_target))
            .with_secret_worker(worker);
        let code = runtime.block_on(serve(deps, &mut signals));
        // Blocking tasks (a registry close, a prune) get a moment to end.
        runtime.shutdown_timeout(RUNTIME_SHUTDOWN);
        // Step 6 of the stop: the registry is closed, so the lock can go.
        release(lock);
        tracing::info!(exit_code = code, "stacklyd stopped");
        code
    }

    /// Steps 9 to 12, then the wait for a signal or a fatal error, then the
    /// stop order.
    async fn serve(deps: DaemonDeps, signals: &mut StopSignals) -> u8 {
        let handle = match server::start(deps).await {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(
                    error = %error,
                    code = error.code(),
                    "cannot start the daemon server",
                );
                return EXIT_START_FAILED;
            }
        };
        let (reason, code) = tokio::select! {
            signal = signals.recv() => {
                tracing::info!(signal, "stacklyd stopping");
                (ShutdownReason::Signal, 0)
            }
            code = handle.fatal() => {
                tracing::error!(exit_code = code, "stacklyd stopping after a fatal error");
                (ShutdownReason::Requested, code)
            }
        };
        let second = async {
            let signal = signals.recv().await;
            tracing::warn!(signal, "second signal");
        };
        let stopped = handle.shutdown(reason, second).await;
        if stopped.forced {
            tracing::warn!("the stop skipped the drain and the flush");
        }
        code
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

    #[test]
    fn an_unset_catalog_dir_means_no_catalog_folder() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let fs = stackly_testkit::FakeFilesystem::new(&root);

        assert_eq!(parse_catalog_dir(&fs, None), Ok(None));
    }

    #[test]
    fn a_catalog_dir_is_kept_as_its_canonical_development_location() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let fs = stackly_testkit::FakeFilesystem::new(&root);
        let dir = root.path().join("catalog");
        std::fs::create_dir(&dir).unwrap();
        let dotted = root.path().join("catalog/../catalog");

        let location = parse_catalog_dir(&fs, Some(dotted.as_os_str())).unwrap();

        assert_eq!(
            location,
            Some(CatalogLocation::Development(
                std::fs::canonicalize(&dir).unwrap()
            ))
        );
    }

    #[test]
    fn an_empty_relative_missing_or_file_catalog_dir_is_refused_with_its_value() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let fs = stackly_testkit::FakeFilesystem::new(&root);
        let file = root.path().join("catalog.json");
        std::fs::write(&file, "{}").unwrap();
        let missing = root.path().join("missing");
        for value in [
            OsStr::new(""),
            OsStr::new("relative/catalog"),
            missing.as_os_str(),
            file.as_os_str(),
        ] {
            let error = parse_catalog_dir(&fs, Some(value)).unwrap_err();

            assert_eq!(
                error.to_string(),
                format!(
                    "STACKLY_CATALOG_DIR {:?} is not an absolute path to a folder",
                    value.to_string_lossy()
                )
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_filter_that_is_not_utf8_is_refused() {
        use std::os::unix::ffi::OsStrExt;

        let error = parse_log_filter(Some(OsStr::from_bytes(b"info\xff"))).unwrap_err();

        assert_eq!(error.reason, "not valid UTF-8");
    }
}
