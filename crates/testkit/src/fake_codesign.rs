//! A scripted code signing port for tests (spec 0018): what `assess`
//! answers per path, which files carry quarantine, and a log of every call.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_platform::{CodeAssessment, CodeSigning, PlatformError, QuarantineInfo, SignatureKind};

/// One call the fake saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigningCall {
    /// `assess` of this path.
    Assess(PathBuf),
    /// `sign_adhoc` of these paths.
    SignAdhoc(Vec<PathBuf>),
    /// `quarantine` of this path.
    Quarantine(PathBuf),
}

#[derive(Debug)]
struct State {
    /// Answers by path suffix, first match wins.
    assessments: Vec<(PathBuf, CodeAssessment)>,
    quarantined: Vec<(PathBuf, QuarantineInfo)>,
    /// Suffixes that still fail strict verification after `sign_adhoc`.
    unsignable: Vec<PathBuf>,
    /// Paths `sign_adhoc` signed, so they assess as ad hoc and valid.
    signed: Vec<PathBuf>,
    default: CodeAssessment,
    sign_error: Option<PlatformError>,
    calls: Vec<SigningCall>,
}

/// A [`CodeSigning`] the test scripts. Clones share one state.
///
/// By default every file is signed ad hoc and verifies, and none carries
/// quarantine. A path rule matches by suffix, so a test names
/// `bin/nats-server` without knowing the scratch folder.
///
/// ```
/// use stackly_platform::{CodeSigning, SignatureKind};
/// use stackly_testkit::FakeCodeSigning;
///
/// let signing = FakeCodeSigning::new();
/// signing.set_unsigned("lib/libssl.3.dylib");
/// let found = signing.assess("/stage/tree/lib/libssl.3.dylib".as_ref())?;
/// assert_eq!(found.signature, SignatureKind::Unsigned);
/// assert!(!found.strict_valid);
/// # Ok::<(), stackly_platform::PlatformError>(())
/// ```
#[derive(Debug, Clone)]
pub struct FakeCodeSigning {
    state: Arc<Mutex<State>>,
}

impl Default for FakeCodeSigning {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeCodeSigning {
    /// A port where every file is signed ad hoc and valid.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                assessments: Vec::new(),
                quarantined: Vec::new(),
                unsignable: Vec::new(),
                signed: Vec::new(),
                default: CodeAssessment {
                    signature: SignatureKind::Adhoc,
                    strict_valid: true,
                    detail: None,
                },
                sign_error: None,
                calls: Vec::new(),
            })),
        }
    }

    /// Answers `assessment` for every path that ends with `suffix`.
    pub fn set_assessment(&self, suffix: impl Into<PathBuf>, assessment: CodeAssessment) {
        self.lock().assessments.push((suffix.into(), assessment));
    }

    /// Makes the files ending with `suffix` unsigned and invalid.
    pub fn set_unsigned(&self, suffix: impl Into<PathBuf>) {
        self.set_assessment(
            suffix,
            CodeAssessment {
                signature: SignatureKind::Unsigned,
                strict_valid: false,
                detail: Some("code object is not signed at all".to_owned()),
            },
        );
    }

    /// Makes the files ending with `suffix` fail strict verification even
    /// after `sign_adhoc`.
    pub fn set_unsignable(&self, suffix: impl Into<PathBuf>) {
        self.lock().unsignable.push(suffix.into());
    }

    /// Makes the files ending with `suffix` carry the quarantine attribute.
    pub fn set_quarantined(&self, suffix: impl Into<PathBuf>, agent: Option<&str>) {
        self.lock().quarantined.push((
            suffix.into(),
            QuarantineInfo {
                agent: agent.map(str::to_owned),
            },
        ));
    }

    /// Makes `sign_adhoc` fail with `error`.
    pub fn fail_signing(&self, error: PlatformError) {
        self.lock().sign_error = Some(error);
    }

    /// Returns every call seen so far, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<SigningCall> {
        self.lock().calls.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl CodeSigning for FakeCodeSigning {
    fn assess(&self, path: &Path) -> Result<CodeAssessment, PlatformError> {
        let mut state = self.lock();
        state.calls.push(SigningCall::Assess(path.to_path_buf()));
        if state.signed.iter().any(|signed| signed == path) {
            let valid = !state.unsignable.iter().any(|suffix| path.ends_with(suffix));
            return Ok(CodeAssessment {
                signature: SignatureKind::Adhoc,
                strict_valid: valid,
                detail: (!valid).then(|| "a signature is invalid".to_owned()),
            });
        }
        let found = state
            .assessments
            .iter()
            .find(|(suffix, _)| path.ends_with(suffix))
            .map_or_else(|| state.default.clone(), |(_, found)| found.clone());
        Ok(found)
    }

    fn sign_adhoc(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        let mut state = self.lock();
        state.calls.push(SigningCall::SignAdhoc(paths.to_vec()));
        if let Some(error) = state.sign_error.clone() {
            return Err(error);
        }
        state.signed.extend(paths.iter().cloned());
        Ok(())
    }

    fn quarantine(&self, path: &Path) -> Result<Option<QuarantineInfo>, PlatformError> {
        let mut state = self.lock();
        state
            .calls
            .push(SigningCall::Quarantine(path.to_path_buf()));
        Ok(state
            .quarantined
            .iter()
            .find(|(suffix, _)| path.ends_with(suffix))
            .map(|(_, info)| info.clone()))
    }
}
