//! Ports that pass every call to the real macOS adapter and remember it
//! (spec 0018 AC-20): which programs the daemon spawned, which files it
//! asked `/usr/bin/codesign` about, and whether it tried to download
//! anything. The adapter has no spawn hook of its own, so the record sits at
//! the port; `MacosCodeSigning` runs `/usr/bin/codesign` and nothing else.
#![allow(dead_code, reason = "each scenario uses a part of these recorders")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use stackly_domain::{BootId, LockState, Pid};
use stackly_platform::{
    ChildRole, CodeAssessment, CodeSigning, ExitListener, FetchError, FetchRequest, FetchSink,
    FetchSummary, Fetcher, FileIdentity, HelperHandle, HoldsFile, LockOutcome, MainHandle,
    PlatformError, ProcessError, ProcessFacts, ProcessSupervision, ProviderSignal, QuarantineInfo,
    SignalTarget, SpawnRequest, SpoolReader, SpoolWriter, SuspendedChild,
};
use stackly_platform_macos::{MacosCodeSigning, MacosProcessSupervision};

/// One process the daemon asked the adapter to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spawned {
    /// The executable.
    pub program: PathBuf,
    /// The arguments after it.
    pub args: Vec<String>,
    /// Why it was started.
    pub role: ChildRole,
    /// The whole environment the child got, in order.
    pub env: Vec<(String, String)>,
}

/// [`MacosProcessSupervision`] that records each spawn.
pub struct RecordingProcesses {
    inner: MacosProcessSupervision,
    spawned: Arc<Mutex<Vec<Spawned>>>,
}

impl RecordingProcesses {
    /// Creates the recorder and the handle a test reads the record with.
    pub fn new() -> (Self, Arc<Mutex<Vec<Spawned>>>) {
        let spawned = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                inner: MacosProcessSupervision::new(),
                spawned: Arc::clone(&spawned),
            },
            spawned,
        )
    }

    fn note(&self, request: &SpawnRequest<'_>) {
        self.spawned
            .lock()
            .expect("no test thread panics")
            .push(Spawned {
                program: request.program.to_path_buf(),
                args: request.args.to_vec(),
                role: request.role,
                env: request.env.to_vec(),
            });
    }
}

impl ProcessSupervision for RecordingProcesses {
    fn spawn_main(&self, request: SpawnRequest<'_>) -> Result<SuspendedChild, ProcessError> {
        self.note(&request);
        self.inner.spawn_main(request)
    }

    fn resume(&self, child: SuspendedChild) -> Result<MainHandle, ProcessError> {
        self.inner.resume(child)
    }

    fn discard(
        &self,
        child: SuspendedChild,
    ) -> Result<Option<stackly_platform::ExitStatus>, ProcessError> {
        self.inner.discard(child)
    }

    fn spawn_helper(&self, request: SpawnRequest<'_>) -> Result<HelperHandle, ProcessError> {
        self.note(&request);
        self.inner.spawn_helper(request)
    }

    fn inspect(&self, pid: Pid) -> Result<Option<ProcessFacts>, ProcessError> {
        self.inner.inspect(pid)
    }

    fn holds_file(&self, pid: Pid, file: FileIdentity) -> Result<HoldsFile, ProcessError> {
        self.inner.holds_file(pid, file)
    }

    fn signal(&self, target: &SignalTarget, signal: ProviderSignal) -> Result<(), ProcessError> {
        self.inner.signal(target, signal)
    }

    fn continue_stopped(&self, target: &SignalTarget) -> Result<(), ProcessError> {
        self.inner.continue_stopped(target)
    }

    fn terminate_helper_group(
        &self,
        helper: &HelperHandle,
        force: bool,
    ) -> Result<(), ProcessError> {
        self.inner.terminate_helper_group(helper, force)
    }

    fn group_members(&self, group: Pid) -> Result<Vec<Pid>, ProcessError> {
        self.inner.group_members(group)
    }

    fn boot_id(&self) -> Result<BootId, ProcessError> {
        self.inner.boot_id()
    }

    fn watch_exit(&self, pid: Pid, listener: Arc<dyn ExitListener>) -> Result<(), ProcessError> {
        self.inner.watch_exit(pid, listener)
    }

    fn lock_instance(&self, path: &Path) -> Result<LockOutcome, ProcessError> {
        self.inner.lock_instance(path)
    }

    fn probe_lock(&self, path: &Path) -> Result<LockState, ProcessError> {
        self.inner.probe_lock(path)
    }

    fn create_spool(&self, path: &Path) -> Result<SpoolWriter, ProcessError> {
        self.inner.create_spool(path)
    }

    fn open_spool(&self, path: &Path) -> Result<Option<Box<dyn SpoolReader>>, ProcessError> {
        self.inner.open_spool(path)
    }
}

/// One question the daemon asked the code signing port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// `assess` of a file.
    Assess(PathBuf),
    /// `sign_adhoc` of these files.
    Sign(Vec<PathBuf>),
    /// `quarantine` of a file.
    Quarantine(PathBuf),
}

/// [`MacosCodeSigning`] that records each call.
pub struct RecordingCodeSigning {
    inner: MacosCodeSigning,
    asked: Arc<Mutex<Vec<Asked>>>,
}

impl RecordingCodeSigning {
    /// Creates the recorder and the handle a test reads the record with.
    pub fn new() -> (Self, Arc<Mutex<Vec<Asked>>>) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                inner: MacosCodeSigning::new(),
                asked: Arc::clone(&asked),
            },
            asked,
        )
    }

    fn note(&self, asked: Asked) {
        self.asked
            .lock()
            .expect("no test thread panics")
            .push(asked);
    }
}

impl CodeSigning for RecordingCodeSigning {
    fn assess(&self, path: &Path) -> Result<CodeAssessment, PlatformError> {
        self.note(Asked::Assess(path.to_path_buf()));
        self.inner.assess(path)
    }

    fn sign_adhoc(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        self.note(Asked::Sign(paths.to_vec()));
        self.inner.sign_adhoc(paths)
    }

    fn quarantine(&self, path: &Path) -> Result<Option<QuarantineInfo>, PlatformError> {
        self.note(Asked::Quarantine(path.to_path_buf()));
        self.inner.quarantine(path)
    }
}

/// A fetcher that refuses every request and remembers its URL, so a test
/// that must install from the cache proves it never asked the network.
pub struct NoNetwork {
    urls: Arc<Mutex<Vec<String>>>,
}

impl NoNetwork {
    /// Creates the fetcher and the handle a test reads the URLs with.
    pub fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
        let urls = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                urls: Arc::clone(&urls),
            },
            urls,
        )
    }
}

impl Fetcher for NoNetwork {
    fn fetch(
        &self,
        request: &FetchRequest<'_>,
        _sink: &mut dyn FetchSink,
    ) -> Result<FetchSummary, FetchError> {
        self.urls
            .lock()
            .expect("no test thread panics")
            .push(request.url.as_str().to_owned());
        Err(FetchError::Unreachable {
            host: "no network in this test".to_owned(),
            detail: "the native install tests install from the cache".to_owned(),
        })
    }
}
