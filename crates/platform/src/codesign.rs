//! The code signing port (spec 0018, *Validasi binary*): read the signature
//! of a Mach-O file, sign files ad hoc, and tell whether a file carries the
//! quarantine attribute. It never runs the code of an artifact.

use std::path::{Path, PathBuf};

use stackly_domain::{Os, TeamId};

use crate::capability::PlatformCapability;
use crate::error::PlatformError;

/// Who signed a file, as `codesign -dv --verbose=2` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignatureKind {
    /// Signed with a Developer ID certificate.
    DeveloperId {
        /// The team of the certificate.
        team: TeamId,
        /// The leaf authority, such as `Developer ID Application: Name (TEAM)`.
        authority: String,
    },
    /// Signed ad hoc, without a certificate.
    Adhoc,
    /// Signed ad hoc by the linker.
    LinkerSigned,
    /// Signed by another authority, such as Apple itself.
    OtherSigned {
        /// The leaf authority.
        authority: String,
    },
    /// No signature.
    Unsigned,
}

/// What [`CodeSigning::assess`] found out about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAssessment {
    /// Who signed it.
    pub signature: SignatureKind,
    /// Whether `codesign --verify --strict` accepts the file.
    pub strict_valid: bool,
    /// What the verification said when it refused, with paths removed.
    pub detail: Option<String>,
}

/// What the quarantine attribute of a file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineInfo {
    /// The name of the agent that set it, from the attribute value, without
    /// its UUID.
    pub agent: Option<String>,
}

/// Signatures and quarantine, the only part of the platform that looks at
/// either.
pub trait CodeSigning: Send + Sync {
    /// Reads the signature of `path` and verifies it strictly.
    ///
    /// # Errors
    ///
    /// Returns a [`PlatformError`] when the tool cannot be run or its output
    /// cannot be read; a file that is unsigned or does not verify is not an
    /// error but an answer.
    fn assess(&self, path: &Path) -> Result<CodeAssessment, PlatformError>;

    /// Signs every file in `paths` ad hoc, replacing any signature, without
    /// preserving metadata.
    ///
    /// # Errors
    ///
    /// Returns a [`PlatformError`] when the tool cannot be run, is stopped
    /// by its time limit, or refuses a file.
    fn sign_adhoc(&self, paths: &[PathBuf]) -> Result<(), PlatformError>;

    /// Reads the quarantine attribute of `path` without following a
    /// symlink, or `None` when there is none.
    ///
    /// # Errors
    ///
    /// Returns a [`PlatformError`] when the attributes cannot be read.
    fn quarantine(&self, path: &Path) -> Result<Option<QuarantineInfo>, PlatformError>;
}

/// The code signing port of an OS without an adapter: every call answers
/// [`PlatformError::NotSupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedCodeSigning {
    os: Os,
}

impl UnsupportedCodeSigning {
    /// Creates the port for `os`.
    #[must_use]
    pub const fn new(os: Os) -> Self {
        Self { os }
    }

    const fn not_supported(&self) -> PlatformError {
        PlatformError::NotSupported {
            capability: PlatformCapability::CodeSigning,
            os: self.os,
        }
    }
}

impl CodeSigning for UnsupportedCodeSigning {
    fn assess(&self, _path: &Path) -> Result<CodeAssessment, PlatformError> {
        Err(self.not_supported())
    }

    fn sign_adhoc(&self, _paths: &[PathBuf]) -> Result<(), PlatformError> {
        Err(self.not_supported())
    }

    fn quarantine(&self, _path: &Path) -> Result<Option<QuarantineInfo>, PlatformError> {
        Err(self.not_supported())
    }
}
