//! Validating an extracted runtime (spec 0018, *Validasi binary*): the
//! executables, the Mach-O files and their closure, the signing policy, the
//! quarantine attribute, and one run of the manifest's `runtime.versionCheck`.
//!
//! Everything works on the staging tree. Nothing here removes quarantine,
//! changes a system setting, or runs a tool other than the platform's
//! `codesign` (through [`CodeSigning`](stackly_platform::CodeSigning)) and
//! the artifact's own executable.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_domain::{ErrorCodeName, ExactVersion, OsVersion, TeamId};
use stackly_platform::{
    CapturedOutput, ChildRole, EntryPath, ExitListener, ExitNotice, ExitStatus, HelperHandle,
    OutputTarget, Platform, PlatformError, ProcessError, SignatureKind, SpawnRequest, StdinSource,
    StopFlag, TreeWriter,
};
use tokio::sync::oneshot;

use super::archive::{EntryKind, EntryRecord};
use super::closure::{self, ClosureError, ClosureInput, MachOEntry};
use super::launch::{LaunchRefusal, classify_early_exit};
use super::macho::{self, MachOError};
use crate::provider::{CHILD_PATH, SigningPolicy, VersionCheck};
use regex::{Regex, RegexBuilder};

/// How long after the time limit a version check may still take to report
/// its end, after its process group was killed.
pub const CHECK_REAP_GRACE: Duration = Duration::from_secs(5);

/// The errno of `EBADARCH` on macOS: the kernel refused the architecture.
const EBADARCH: i32 = 86;

/// The most bytes of the stderr tail kept in an error.
const STDERR_TAIL_MAX: usize = 512;

/// The name of the marker file at the root of a staged runtime.
pub const MARKER_FILE: &str = ".stackly-runtime.json";

/// The longest version check pattern, in characters (the pattern limit of
/// spec 0009).
const PATTERN_MAX: usize = 512;

/// The compiled size limit of a version check pattern, in bytes.
const PATTERN_SIZE_LIMIT: usize = 64 * 1024;

/// A version check frozen as plain data: the manifest's
/// `runtime.versionCheck`, copied into the journal input of an install so a
/// resumed operation never reads the catalog again.
#[derive(Debug, Clone)]
pub struct VersionCheckSpec {
    args: Vec<String>,
    pattern: Regex,
    timeout: Duration,
}

/// Why a version check could not be built from plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "the version check pattern is empty, too long, does not compile, or has no single capture group"
)]
pub struct VersionCheckSpecError;

impl VersionCheckSpec {
    /// Builds the check, compiling `pattern` within the pattern limits of
    /// the manifest and requiring exactly one capture group.
    ///
    /// # Errors
    ///
    /// Returns [`VersionCheckSpecError`] for a pattern that breaks those
    /// rules.
    pub fn new(
        args: Vec<String>,
        pattern: &str,
        timeout: Duration,
    ) -> Result<Self, VersionCheckSpecError> {
        if pattern.is_empty() || pattern.chars().count() > PATTERN_MAX {
            return Err(VersionCheckSpecError);
        }
        let regex = RegexBuilder::new(pattern)
            .size_limit(PATTERN_SIZE_LIMIT)
            .build()
            .map_err(|_| VersionCheckSpecError)?;
        if regex.captures_len() != 2 {
            return Err(VersionCheckSpecError);
        }
        Ok(Self {
            args,
            pattern: regex,
            timeout,
        })
    }

    /// Copies a version check of a validated package.
    #[must_use]
    pub fn from_check(check: &VersionCheck) -> Self {
        Self::new(check.args().to_vec(), check.pattern(), check.timeout())
            .expect("a pattern the manifest accepted passes the same limits")
    }

    /// Returns the literal arguments.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Returns the pattern text.
    #[must_use]
    pub fn pattern(&self) -> &str {
        self.pattern.as_str()
    }

    /// Returns the time limit.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Searches every line of `stdout`, then of `stderr`, with the trailing
    /// `\n` and `\r` removed, and returns the capture group of the first
    /// line the pattern matches.
    #[must_use]
    pub fn read_version<'a>(&self, stdout: &'a str, stderr: &'a str) -> Option<&'a str> {
        stdout
            .lines()
            .chain(stderr.lines())
            .map(|line| line.trim_end_matches(['\r', '\n']))
            .find_map(|line| {
                self.pattern
                    .captures(line)
                    .and_then(|captures| captures.get(1))
                    .map(|found| found.as_str())
            })
    }
}

/// What validation looks at.
pub struct ValidateInput<'a> {
    /// The platform ports.
    pub platform: &'a Platform,
    /// The staging tree, an absolute path.
    pub tree: &'a Path,
    /// A scratch folder for the version check, `scratch/check`.
    pub check_dir: &'a Path,
    /// Every entry of the tree, from the extract report.
    pub entries: &'a [EntryRecord],
    /// The artifact's executables.
    pub executables: &'a [EntryPath],
    /// The lowest macOS the artifact says it needs.
    pub min_os: OsVersion,
    /// What happens to the code signatures.
    pub signing: SigningPolicy,
    /// True for a maintainer artifact.
    pub maintainer: bool,
    /// The file names of the `bundled` and `own` dependencies, or `None`
    /// when the catalog lists none.
    pub expected_libraries: Option<&'a [String]>,
    /// The version the artifact claims.
    pub version: &'a ExactVersion,
    /// The version check of the provider package, if it declares one.
    pub version_check: Option<&'a VersionCheckSpec>,
    /// Raised to stop at the next file or pause.
    pub stop: &'a StopFlag,
}

/// One executable and what is known about its signature, for
/// `signature_info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableSignature {
    /// Where it lies in the tree.
    pub path: String,
    /// Who signed it.
    pub kind: SignatureCode,
    /// The team ID of a Developer ID signature.
    pub team_id: Option<TeamId>,
}

/// The kind of signature, as `signature_info` writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureCode {
    /// A Developer ID certificate.
    DeveloperId,
    /// Ad hoc.
    Adhoc,
    /// Ad hoc by the linker.
    LinkerSigned,
    /// Another authority.
    Other,
    /// Not signed.
    Unsigned,
}

impl SignatureCode {
    /// Returns the code `signature_info` stores.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::DeveloperId => "developer_id",
            Self::Adhoc => "adhoc",
            Self::LinkerSigned => "linker_signed",
            Self::Other => "other",
            Self::Unsigned => "unsigned",
        }
    }
}

/// What validation found out, for the registry and the checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ValidateReport {
    /// The signing policy that was applied.
    pub signing: SigningPolicy,
    /// Whether the install signed every Mach-O file ad hoc.
    pub adhoc_signed: bool,
    /// How many Mach-O files the tree holds.
    pub mach_o_files: u64,
    /// The signature of each executable.
    pub executables: Vec<ExecutableSignature>,
    /// Findings that do not fail the install, sorted.
    pub warnings: Vec<String>,
}

/// Why validation failed, one variant per error code of spec 0018.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValidateError {
    /// An `executables` path is missing or not a regular file.
    #[error("executable {path} is {reason}")]
    ExecutableMissing {
        /// The path.
        path: String,
        /// `missing` or `not_regular`.
        reason: &'static str,
    },
    /// An executable has no arm64 slice, or the kernel refused its
    /// architecture.
    #[error(
        "{path} is not built for Apple Silicon (found {arches:?}), and Stackly does not run it under Rosetta"
    )]
    ArchitectureMismatch {
        /// The executable.
        path: String,
        /// The architectures it has.
        arches: Vec<String>,
    },
    /// A binary needs a newer macOS.
    #[error("{path} needs macOS {required}, this Mac runs {host}")]
    OsTooOld {
        /// The file.
        path: String,
        /// What it needs.
        required: OsVersion,
        /// What this Mac runs.
        host: OsVersion,
    },
    /// A library resolves outside the runtime.
    #[error("{path} loads {library} from outside the runtime")]
    DependencyOutsideRuntime {
        /// The file that loads it.
        path: String,
        /// The install name.
        library: String,
    },
    /// The libraries found differ from the catalog's dependencies.
    #[error(
        "the libraries differ from the dependencies: missing {missing:?}, unexpected {unexpected:?}"
    )]
    DependencyMismatch {
        /// Listed but not found.
        missing: Vec<String>,
        /// Found but not listed.
        unexpected: Vec<String>,
    },
    /// The signing policy is not met.
    #[error("{path} does not satisfy the {policy} signing policy")]
    SignatureInvalid {
        /// The file.
        path: String,
        /// The policy code.
        policy: &'static str,
        /// What `codesign` said, with paths removed.
        detail: String,
    },
    /// An executable carries the quarantine attribute.
    #[error("{path} is quarantined")]
    Quarantined {
        /// The executable.
        path: String,
    },
    /// macOS killed the version check with a signal the daemon did not
    /// send.
    #[error("macOS killed {path} with signal {signal}")]
    BinaryRejectedByOs {
        /// The executable.
        path: String,
        /// The signal.
        signal: i32,
    },
    /// The version check did not finish in time.
    #[error("the version check of {path} did not finish in {seconds} seconds")]
    BinaryCheckTimeout {
        /// The executable.
        path: String,
        /// The time limit.
        seconds: u64,
    },
    /// The version check exited non zero, died of another signal, could not
    /// start, or a Mach-O file could not be read.
    #[error("the check of {path} failed")]
    BinaryCheckFailed {
        /// The file.
        path: String,
        /// The exit code, when it exited.
        exit_code: Option<i32>,
        /// The signal, when one ended it.
        signal: Option<i32>,
        /// The errno of a failed spawn.
        errno: Option<i32>,
        /// The tail of stderr, shortened and without the tree path.
        stderr: Option<String>,
    },
    /// The version the binary reports is not the catalog's.
    #[error("the binary reports version {observed:?}, expected {expected}")]
    VersionMismatch {
        /// The catalog's version.
        expected: String,
        /// What the pattern read, if it matched.
        observed: Option<String>,
    },
    /// The volume ran out of room.
    #[error("the volume has no room left")]
    InsufficientSpace,
    /// A platform call failed.
    #[error("{op} failed: {detail}")]
    Io {
        /// The step.
        op: &'static str,
        /// What the platform said.
        detail: String,
    },
    /// The stop flag was raised.
    #[error("validation was stopped")]
    Stopped,
}

impl ValidateError {
    /// Returns the catalog code of the error.
    #[must_use]
    pub fn code(&self) -> ErrorCodeName {
        use super::codes as c;
        match self {
            Self::ExecutableMissing { .. } => c::EXECUTABLE_MISSING,
            Self::ArchitectureMismatch { .. } => c::ARCHITECTURE_MISMATCH,
            Self::OsTooOld { .. } => c::OS_TOO_OLD,
            Self::DependencyOutsideRuntime { .. } => c::DEPENDENCY_OUTSIDE_RUNTIME,
            Self::DependencyMismatch { .. } => c::DEPENDENCY_MISMATCH,
            Self::SignatureInvalid { .. } => c::SIGNATURE_INVALID,
            Self::Quarantined { .. } => c::QUARANTINED,
            Self::BinaryRejectedByOs { .. } => c::BINARY_REJECTED_BY_OS,
            Self::BinaryCheckTimeout { .. } => c::BINARY_CHECK_TIMEOUT,
            Self::BinaryCheckFailed { .. } => c::BINARY_CHECK_FAILED,
            Self::VersionMismatch { .. } => c::VERSION_MISMATCH,
            Self::InsufficientSpace => c::INSUFFICIENT_SPACE,
            Self::Io { .. } | Self::Stopped => c::IO_FAILED,
        }
    }
}

/// The marker file at the root of a staged runtime (spec 0018, *Commit
/// atomik*): it names the operation that staged it, so a crash between the
/// rename and the registry batch is recognized, and a folder left by an
/// operation that never committed is recognized as an orphan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The operation that staged the tree.
    pub operation_id: String,
    /// The SHA-256 of the artifact.
    pub sha256: String,
    /// The software.
    pub software: String,
    /// The exact version.
    pub version: String,
    /// The target, as text.
    pub target: String,
}

impl Marker {
    /// Returns the JSON text of the marker file.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "schema": "stackly.runtime-marker/v1",
            "operationId": self.operation_id,
            "sha256": self.sha256,
            "software": self.software,
            "version": self.version,
            "target": self.target,
        })
        .to_string()
    }
}

/// Finishes the staged tree through the writer that built it: the marker
/// file, then the final mode of every entry, then one sync.
///
/// The tree writer needs an empty root, so only the extract step holds one;
/// it calls this right after extraction. Folders end 0755, a file the
/// archive made executable or an `executables` path 0555, any other file
/// 0444, and the marker 0444. `codesign` still signs a file with these
/// modes, so the order of the spec (signing first) changes nothing.
///
/// # Errors
///
/// Returns the [`PlatformError`] of the first write or mode change that
/// failed.
pub fn finish_tree(
    sink: &mut dyn TreeWriter,
    entries: &[EntryRecord],
    marker: &Marker,
) -> Result<(), PlatformError> {
    let marker_path = EntryPath::new(MARKER_FILE).map_err(|_| PlatformError::Io {
        op: "write the marker",
        path: PathBuf::from(MARKER_FILE),
        kind: std::io::ErrorKind::InvalidInput,
        message: "the marker name is not an entry path".to_owned(),
    })?;
    let mut file = sink.create_file(&marker_path)?;
    std::io::Write::write_all(&mut file, marker.to_json().as_bytes())
        .map_err(|err| PlatformError::from_io("write the marker", Path::new(MARKER_FILE), &err))?;
    file.finish()?;
    if let Some(mode) = stackly_platform::FileMode::new(0o444) {
        sink.set_mode(&marker_path, mode)?;
    }
    for entry in entries {
        if let Some(mode) = entry.mode {
            sink.set_mode(&entry.path, mode)?;
        }
    }
    sink.sync()
}

/// Validates the staged runtime.
///
/// `progress` is called with how many Mach-O files were checked (the exec
/// check counts as one more) and how many there are to check.
///
/// # Errors
///
/// Returns the first [`ValidateError`], in the order of the spec: the
/// executables, the Mach-O files with architecture, macOS minimum, closure,
/// and dependencies, the signing policy, quarantine, and the version check.
pub async fn validate(
    input: ValidateInput<'_>,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<ValidateReport, ValidateError> {
    let host = input.platform.info().host_facts();
    check_executables(&input)?;
    let machos = scan_machos(&input, progress).await?;
    let total = machos.len() as u64 + u64::from(input.version_check.is_some());
    if input.min_os > host.os_version {
        return Err(ValidateError::OsTooOld {
            path: "artifact".to_owned(),
            required: input.min_os,
            host: host.os_version,
        });
    }
    let report = closure::check(&ClosureInput::new(
        input.entries,
        &machos,
        input.executables,
        &host.os_version,
        input.maintainer,
        input.expected_libraries,
    ))
    .map_err(closure_error)?;
    let mut warnings = report.warnings;
    let (adhoc_signed, executables) = apply_signing(&input, &machos).await?;
    check_quarantine(&input).await?;
    match (input.version_check, input.executables.first()) {
        (Some(check), Some(executable)) => {
            exec_check(&input, check, executable).await?;
            progress(total, total);
        }
        _ => warnings.push("version check not declared".to_owned()),
    }
    warnings.sort();
    Ok(ValidateReport {
        signing: input.signing,
        adhoc_signed,
        mach_o_files: machos.len() as u64,
        executables,
        warnings,
    })
}

fn check_executables(input: &ValidateInput<'_>) -> Result<(), ValidateError> {
    for executable in input.executables {
        let entry = input
            .entries
            .iter()
            .find(|entry| entry.path == *executable)
            .ok_or_else(|| ValidateError::ExecutableMissing {
                path: executable.to_string(),
                reason: "missing",
            })?;
        if entry.kind != EntryKind::File {
            return Err(ValidateError::ExecutableMissing {
                path: executable.to_string(),
                reason: "not_regular",
            });
        }
    }
    Ok(())
}

fn io_error(op: &'static str, err: &PlatformError) -> ValidateError {
    match err {
        PlatformError::NoSpace { .. } => ValidateError::InsufficientSpace,
        _ => ValidateError::Io {
            op,
            detail: err.to_string(),
        },
    }
}

fn closure_error(error: ClosureError) -> ValidateError {
    match error {
        ClosureError::ArchitectureMismatch { path, arches } => {
            ValidateError::ArchitectureMismatch { path, arches }
        }
        ClosureError::OsTooOld {
            path,
            required,
            host,
        } => ValidateError::OsTooOld {
            path,
            required,
            host,
        },
        ClosureError::DependencyOutsideRuntime { path, library } => {
            ValidateError::DependencyOutsideRuntime { path, library }
        }
        ClosureError::DependencyMismatch {
            missing,
            unexpected,
        } => ValidateError::DependencyMismatch {
            missing,
            unexpected,
        },
    }
}

/// Reads the first 8 bytes of every regular file and parses the Mach-O ones.
async fn scan_machos(
    input: &ValidateInput<'_>,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Vec<MachOEntry>, ValidateError> {
    let platform = input.platform.clone();
    let tree = input.tree.to_path_buf();
    let files: Vec<EntryPath> = input
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .map(|entry| entry.path.clone())
        .collect();
    let stop = input.stop.clone();
    let extra = u64::from(input.version_check.is_some());
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<(u64, u64)>();
    let work = tokio::task::spawn_blocking(move || {
        let mut found = Vec::new();
        let mut heads = Vec::new();
        for path in files {
            if stop.is_stopped() {
                return Err(ValidateError::Stopped);
            }
            let full = tree.join(path.as_path());
            let mut file = platform
                .fs()
                .open_read(&full)
                .map_err(|err| io_error("read a staged file", &err))?;
            let mut head = [0_u8; 8];
            let read = std::io::Read::read(&mut file, &mut head).unwrap_or(0);
            if macho::classify(&head[..read]).is_some() {
                heads.push((path, full));
            }
        }
        let total = heads.len() as u64 + extra;
        for (done, (path, full)) in heads.into_iter().enumerate() {
            if stop.is_stopped() {
                return Err(ValidateError::Stopped);
            }
            let mut file = platform
                .fs()
                .open_read(&full)
                .map_err(|err| io_error("read a staged file", &err))?;
            let parsed = macho::parse(&mut *file).map_err(|err| macho_error(&path, &err))?;
            found.push(MachOEntry::new(path, parsed));
            let _ = sender.send((done as u64 + 1, total));
        }
        Ok(found)
    });
    while let Some((done, total)) = receiver.recv().await {
        progress(done, total);
    }
    work.await.map_err(|err| ValidateError::Io {
        op: "scan the staged files",
        detail: err.to_string(),
    })?
}

fn macho_error(path: &EntryPath, error: &MachOError) -> ValidateError {
    ValidateError::BinaryCheckFailed {
        path: path.to_string(),
        exit_code: None,
        signal: None,
        errno: None,
        stderr: Some(error.to_string()),
    }
}

/// Applies the signing policy to every Mach-O file of the tree.
async fn apply_signing(
    input: &ValidateInput<'_>,
    machos: &[MachOEntry],
) -> Result<(bool, Vec<ExecutableSignature>), ValidateError> {
    let platform = input.platform.clone();
    let tree = input.tree.to_path_buf();
    let paths: Vec<(EntryPath, PathBuf)> = machos
        .iter()
        .map(|entry| (entry.path.clone(), tree.join(entry.path.as_path())))
        .collect();
    let executables: Vec<EntryPath> = input.executables.to_vec();
    let policy = input.signing;
    let stop = input.stop.clone();
    let tree_text = input.tree.display().to_string();
    tokio::task::spawn_blocking(move || {
        let assess_all = |paths: &[(EntryPath, PathBuf)]| -> Result<Vec<_>, ValidateError> {
            paths
                .iter()
                .map(|(entry, full)| {
                    if stop.is_stopped() {
                        return Err(ValidateError::Stopped);
                    }
                    platform
                        .code_signing()
                        .assess(full)
                        .map(|found| (entry.clone(), found))
                        .map_err(|err| io_error("assess a signature", &err))
                })
                .collect()
        };
        let invalid = |found: &Vec<(EntryPath, stackly_platform::CodeAssessment)>| {
            found
                .iter()
                .find(|(_, assessment)| !assessment.strict_valid)
                .map(|(entry, assessment)| ValidateError::SignatureInvalid {
                    path: entry.to_string(),
                    policy: policy_code(policy),
                    detail: assessment
                        .detail
                        .clone()
                        .unwrap_or_default()
                        .replace(&tree_text, "<runtime>"),
                })
        };
        let mut found = assess_all(&paths)?;
        let mut adhoc_signed = false;
        if let Some(error) = invalid(&found) {
            if policy == SigningPolicy::AdhocPipeline {
                return Err(error);
            }
            let targets: Vec<PathBuf> = paths.iter().map(|(_, full)| full.clone()).collect();
            platform
                .code_signing()
                .sign_adhoc(&targets)
                .map_err(|err| io_error("sign ad hoc", &err))?;
            adhoc_signed = true;
            found = assess_all(&paths)?;
            if let Some(error) = invalid(&found) {
                return Err(error);
            }
        }
        let signatures = executables
            .iter()
            .filter_map(|executable| {
                found
                    .iter()
                    .find(|(entry, _)| entry == executable)
                    .map(|(entry, assessment)| signature_of(entry, &assessment.signature))
            })
            .collect();
        Ok((adhoc_signed, signatures))
    })
    .await
    .map_err(|err| ValidateError::Io {
        op: "apply the signing policy",
        detail: err.to_string(),
    })?
}

const fn policy_code(policy: SigningPolicy) -> &'static str {
    policy.as_code()
}

fn signature_of(path: &EntryPath, signature: &SignatureKind) -> ExecutableSignature {
    let (kind, team_id) = match signature {
        SignatureKind::DeveloperId { team, .. } => (SignatureCode::DeveloperId, Some(team.clone())),
        SignatureKind::Adhoc => (SignatureCode::Adhoc, None),
        SignatureKind::LinkerSigned => (SignatureCode::LinkerSigned, None),
        SignatureKind::Unsigned => (SignatureCode::Unsigned, None),
        _ => (SignatureCode::Other, None),
    };
    ExecutableSignature {
        path: path.to_string(),
        kind,
        team_id,
    }
}

/// Refuses an executable with the quarantine attribute, without running it.
async fn check_quarantine(input: &ValidateInput<'_>) -> Result<(), ValidateError> {
    let platform = input.platform.clone();
    let tree = input.tree.to_path_buf();
    let executables: Vec<EntryPath> = input.executables.to_vec();
    tokio::task::spawn_blocking(move || {
        for executable in executables {
            let full = tree.join(executable.as_path());
            match platform.code_signing().quarantine(&full) {
                Ok(None) => {}
                Ok(Some(_)) => {
                    return Err(ValidateError::Quarantined {
                        path: executable.to_string(),
                    });
                }
                Err(err) => return Err(io_error("read the quarantine attribute", &err)),
            }
        }
        Ok(())
    })
    .await
    .map_err(|err| ValidateError::Io {
        op: "check quarantine",
        detail: err.to_string(),
    })?
}

/// Runs the version check once from the staging tree.
async fn exec_check(
    input: &ValidateInput<'_>,
    check: &VersionCheckSpec,
    executable: &EntryPath,
) -> Result<(), ValidateError> {
    let platform = input.platform.clone();
    let check_dir = input.check_dir.to_path_buf();
    let home = check_dir.join("home");
    let tmp = check_dir.join("tmp");
    let program_in = input.tree.join(executable.as_path());
    let args: Vec<String> = check.args().to_vec();
    let env = vec![
        ("PATH".to_owned(), CHILD_PATH.to_owned()),
        ("HOME".to_owned(), home.display().to_string()),
        ("TMPDIR".to_owned(), tmp.display().to_string()),
        ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
    ];
    let (sender, mut exit) = oneshot::channel();
    let listener = Arc::new(ExitSender(Mutex::new(Some(sender))));
    let spawner = platform.clone();
    let name = executable.to_string();
    let spawned = tokio::task::spawn_blocking(move || {
        for dir in [&check_dir, &home, &tmp] {
            spawner
                .fs()
                .create_dir_private(dir)
                .map_err(|err| io_error("create the check folder", &err))?;
        }
        let program = spawner
            .fs()
            .canonicalize(&program_in)
            .map_err(|err| io_error("resolve the executable", &err))?;
        let processes = spawner.processes();
        let handle = processes
            .spawn_helper(SpawnRequest {
                program: &program,
                args: &args,
                env: &env,
                cwd: &check_dir,
                stdin: StdinSource::Null,
                output: OutputTarget::Capture,
                lock: None,
                role: ChildRole::RuntimeCheck,
            })
            .map_err(|err| spawn_error(&name, &err))?;
        if let Err(error) = processes.watch_exit(handle.pid(), listener) {
            let _ = processes.terminate_helper_group(&handle, true);
            return Err(ValidateError::Io {
                op: "watch the version check",
                detail: error.to_string(),
            });
        }
        Ok(handle)
    })
    .await
    .map_err(|err| ValidateError::Io {
        op: "start the version check",
        detail: err.to_string(),
    })??;

    let limit = check.timeout();
    let notice = wait_for_exit(&platform, &spawned, &mut exit, limit, input.stop).await;
    let path = executable.to_string();
    let notice = match notice {
        Waited::Exited(notice) => notice,
        Waited::TimedOut => {
            return Err(ValidateError::BinaryCheckTimeout {
                path,
                seconds: limit.as_secs(),
            });
        }
        Waited::Stopped => return Err(ValidateError::Stopped),
        Waited::Lost => {
            return Err(ValidateError::Io {
                op: "wait for the version check",
                detail: "the exit watcher dropped its notice".to_owned(),
            });
        }
    };
    let captured = notice.captured.clone().unwrap_or_default();
    let tree_text = input.tree.display().to_string();
    match notice.status {
        Some(ExitStatus::Exited(0)) => read_version(check, input.version, &captured),
        Some(ExitStatus::Exited(code)) => Err(ValidateError::BinaryCheckFailed {
            path,
            exit_code: Some(code),
            signal: None,
            errno: None,
            stderr: Some(stderr_tail(&captured, &tree_text)),
        }),
        status => match classify_early_exit(status, false) {
            Some(LaunchRefusal::BinaryRejectedByOs { signal }) => {
                Err(ValidateError::BinaryRejectedByOs { path, signal })
            }
            _ => Err(ValidateError::BinaryCheckFailed {
                path,
                exit_code: None,
                signal: match status {
                    Some(ExitStatus::Signaled(signal)) => Some(signal),
                    _ => None,
                },
                errno: None,
                stderr: Some(stderr_tail(&captured, &tree_text)),
            }),
        },
    }
}

fn spawn_error(path: &str, error: &ProcessError) -> ValidateError {
    match error {
        ProcessError::SpawnFailed { errno, .. } if *errno == EBADARCH => {
            ValidateError::ArchitectureMismatch {
                path: path.to_owned(),
                arches: Vec::new(),
            }
        }
        ProcessError::SpawnFailed { errno, .. } => ValidateError::BinaryCheckFailed {
            path: path.to_owned(),
            exit_code: None,
            signal: None,
            errno: Some(*errno),
            stderr: None,
        },
        other => ValidateError::Io {
            op: "spawn the version check",
            detail: other.to_string(),
        },
    }
}

fn read_version(
    check: &VersionCheckSpec,
    expected: &ExactVersion,
    captured: &CapturedOutput,
) -> Result<(), ValidateError> {
    let stdout = String::from_utf8_lossy(&captured.stdout);
    let stderr = String::from_utf8_lossy(&captured.stderr);
    let observed = check.read_version(&stdout, &stderr);
    if observed == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(ValidateError::VersionMismatch {
            expected: expected.to_string(),
            observed: observed.map(str::to_owned),
        })
    }
}

/// The last bytes of stderr, shortened on a character boundary, without
/// control characters and without the staging path.
fn stderr_tail(captured: &CapturedOutput, tree: &str) -> String {
    let text = String::from_utf8_lossy(&captured.stderr);
    let text = text.replace(tree, "<runtime>");
    let mut start = text.len().saturating_sub(STDERR_TAIL_MAX);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..]
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

enum Waited {
    Exited(ExitNotice),
    TimedOut,
    Stopped,
    Lost,
}

/// Waits for the check to exit within `limit`. Past it the whole process
/// group is killed at once, with no `SIGTERM` and no grace, and the exit is
/// awaited [`CHECK_REAP_GRACE`] more.
async fn wait_for_exit(
    platform: &Platform,
    handle: &HelperHandle,
    exit: &mut oneshot::Receiver<ExitNotice>,
    limit: Duration,
    stop: &StopFlag,
) -> Waited {
    let stopped = async {
        loop {
            if stop.is_stopped() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let first = tokio::select! {
        notice = &mut *exit => Some(notice),
        () = tokio::time::sleep(limit) => None,
        () = stopped => {
            kill_group(platform, handle).await;
            return Waited::Stopped;
        }
    };
    match first {
        Some(Ok(notice)) => Waited::Exited(notice),
        Some(Err(_)) => Waited::Lost,
        None => {
            kill_group(platform, handle).await;
            let _ = tokio::time::timeout(CHECK_REAP_GRACE, &mut *exit).await;
            Waited::TimedOut
        }
    }
}

async fn kill_group(platform: &Platform, handle: &HelperHandle) {
    let platform = platform.clone();
    let pid = handle.pid();
    let role = handle.role();
    let handle = HelperHandle::new(pid, role);
    let result = tokio::task::spawn_blocking(move || {
        platform.processes().terminate_helper_group(&handle, true)
    })
    .await;
    if let Ok(Err(error)) = result
        && !matches!(error, ProcessError::NotFound { .. })
    {
        tracing::warn!(%pid, %error, "killing the version check group failed");
    }
}

/// Hands one exit notice to the waiting task.
struct ExitSender(Mutex<Option<oneshot::Sender<ExitNotice>>>);

impl ExitListener for ExitSender {
    fn exited(&self, notice: ExitNotice) {
        let sender = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(sender) = sender {
            let _ = sender.send(notice);
        }
    }
}
