//! What the registry keeps about an installed artifact beyond its identity
//! (spec 0005, "Tambahan fitur 18"; spec 0018, "Data model"): the signature
//! and dependency facts, and the compatibility records copied from the
//! catalog at install time.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;

use crate::code::code_enum;
use crate::error::{ValueError, ValueKind};
use crate::os_version::OsVersion;
use crate::provider::ProviderRevisionKey;
use crate::target::{Arch, Os};

/// Most codes in `checks` or `known_limitations` of one record.
pub const CODES_MAX: usize = 64;

/// Longest code in `checks` or `known_limitations`, in bytes.
const CODE_MAX: usize = 64;

/// Longest `summary` of a record, in characters.
const SUMMARY_MAX: usize = 256;

/// Longest `evidence` path of a record, in bytes.
const EVIDENCE_MAX: usize = 256;

/// Longest `os_build` of a record, in bytes.
const OS_BUILD_MAX: usize = 32;

code_enum! {
    /// What a compatibility record was measured against (spec 0018,
    /// "Compatibility record").
    pub enum CompatibilityScope: CompatibilityScope {
        /// The runtime alone, without a provider.
        Runtime => "runtime",
        /// The runtime together with one provider revision.
        Provider => "provider",
    }
}

code_enum! {
    /// The outcome a compatibility record states.
    pub enum CompatibilityResult: CompatibilityResult {
        /// Every check passed.
        Pass => "pass",
        /// At least one check failed.
        Fail => "fail",
    }
}

macro_rules! json_object_text {
    ($(#[$meta:meta])* $name:ident, $kind:ident, $what:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Wraps the JSON object text.
            ///
            /// # Errors
            ///
            /// Returns a [`ValueError`] when the text does not start with
            /// `{` and end with `}`.
            pub fn new(text: String) -> Result<Self, ValueError> {
                if text.len() >= 2 && text.starts_with('{') && text.ends_with('}') {
                    Ok(Self(text))
                } else {
                    Err(ValueError::new(
                        ValueKind::$kind,
                        concat!($what, " must be JSON object text, from `{` to `}`"),
                    ))
                }
            }

            /// Returns the JSON text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = ValueError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::new(s.to_owned())
            }
        }
    };
}

json_object_text! {
    /// The `signature_info` column of an artifact: canonical JSON object
    /// text that core builds and reads (the policy, whether the install
    /// signed ad hoc, and the signature of each executable).
    ///
    /// The domain has no JSON parser, so it checks only the outer shape; the
    /// column's JSON object check is the last guard.
    ///
    /// ```
    /// use stackly_domain::SignatureInfoJson;
    ///
    /// let info = SignatureInfoJson::new(r#"{"machOFiles":3}"#.to_owned())?;
    /// assert_eq!(info.as_str(), r#"{"machOFiles":3}"#);
    /// assert!(SignatureInfoJson::new("[]".to_owned()).is_err());
    /// # Ok::<(), stackly_domain::ValueError>(())
    /// ```
    SignatureInfoJson, SignatureInfoJson, "signature info"
}

json_object_text! {
    /// The `dependency_manifest` column of an artifact: canonical JSON object
    /// text that core builds and reads (the dependencies of the catalog entry
    /// and the warnings of the validation).
    ///
    /// ```
    /// use stackly_domain::DependencyManifestJson;
    ///
    /// let manifest = DependencyManifestJson::new(r#"{"dependencies":[]}"#.to_owned())?;
    /// assert_eq!(manifest.to_string(), r#"{"dependencies":[]}"#);
    /// # Ok::<(), stackly_domain::ValueError>(())
    /// ```
    DependencyManifestJson, DependencyManifestJson, "dependency manifest"
}

/// One row of `compatibility_records`: what a native run found for one
/// artifact on one OS, copied from the catalog when the artifact was
/// installed.
///
/// The source URL, the digest, and the dependencies belong to the artifact
/// the record hangs on, so the record does not repeat them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompatibilityRecordRow {
    /// What the record was measured against.
    pub scope: CompatibilityScope,
    /// The provider revision it was measured with: present exactly for the
    /// `provider` scope.
    pub provider_revision: Option<ProviderRevisionKey>,
    /// The OS of the host the run used.
    pub os: Os,
    /// The OS version of that host.
    pub os_version: OsVersion,
    /// The OS build of that host, such as `26A428`, if known.
    pub os_build: Option<String>,
    /// The architecture of that host.
    pub arch: Arch,
    /// Whether the run passed.
    pub result: CompatibilityResult,
    /// The codes of the checks the run covered, in order.
    pub checks: Vec<String>,
    /// The codes of what the run did not cover.
    pub known_limitations: Vec<String>,
    /// A short summary of the run, if it has one.
    pub summary: Option<String>,
    /// When the run finished.
    pub tested_at: Timestamp,
    /// The evidence folder, relative to `evidence/`, such as
    /// `runtime/<runId>/<recordId>`.
    pub evidence: String,
}

impl CompatibilityRecordRow {
    /// Bundles the columns of one compatibility record.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when `provider_revision` is present exactly
    /// when `scope` is not `provider`, when a code is not 1 to 64 bytes of
    /// `[a-z][a-z0-9_]*` or there are more than 64 of them, when `os_build` is
    /// not 1 to 32 alphanumeric bytes, when `summary` is empty or over 256
    /// characters, or when `evidence` is not 1 to 256 bytes shaped
    /// `<folder>/<path>` without `..`.
    #[expect(
        clippy::too_many_arguments,
        reason = "one argument per column of compatibility_records"
    )]
    pub fn new(
        scope: CompatibilityScope,
        provider_revision: Option<ProviderRevisionKey>,
        os: Os,
        os_version: OsVersion,
        os_build: Option<String>,
        arch: Arch,
        result: CompatibilityResult,
        checks: Vec<String>,
        known_limitations: Vec<String>,
        summary: Option<String>,
        tested_at: Timestamp,
        evidence: String,
    ) -> Result<Self, ValueError> {
        let invalid = |reason| ValueError::new(ValueKind::CompatibilityRecord, reason);
        if (scope == CompatibilityScope::Provider) != provider_revision.is_some() {
            return Err(invalid(
                "a provider revision is required exactly for the provider scope",
            ));
        }
        for codes in [&checks, &known_limitations] {
            if codes.len() > CODES_MAX {
                return Err(invalid("a record lists at most 64 codes per list"));
            }
            if !codes.iter().all(|code| is_code(code)) {
                return Err(invalid(
                    "a code is 1 to 64 bytes: a lowercase letter, then a-z, 0-9, and _",
                ));
            }
        }
        if let Some(build) = &os_build
            && (build.is_empty()
                || build.len() > OS_BUILD_MAX
                || !build.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        {
            return Err(invalid("an OS build is 1 to 32 alphanumeric bytes"));
        }
        if let Some(text) = &summary
            && (text.is_empty() || text.chars().count() > SUMMARY_MAX)
        {
            return Err(invalid("a summary is 1 to 256 characters"));
        }
        if evidence.is_empty()
            || evidence.len() > EVIDENCE_MAX
            || !evidence.starts_with(|c: char| c.is_ascii_lowercase())
            || !evidence.contains('/')
            || evidence.contains("..")
        {
            return Err(invalid(
                "evidence is 1 to 256 bytes shaped <folder>/<path>, without ..",
            ));
        }
        Ok(Self {
            scope,
            provider_revision,
            os,
            os_version,
            os_build,
            arch,
            result,
            checks,
            known_limitations,
            summary,
            tested_at,
            evidence,
        })
    }
}

fn is_code(code: &str) -> bool {
    code.len() <= CODE_MAX
        && code.starts_with(|c: char| c.is_ascii_lowercase())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> crate::Sha256Digest {
        crate::Sha256Digest::from_bytes([7; 32])
    }

    fn row(
        scope: CompatibilityScope,
        revision: Option<ProviderRevisionKey>,
    ) -> Result<CompatibilityRecordRow, ValueError> {
        CompatibilityRecordRow::new(
            scope,
            revision,
            Os::Macos,
            OsVersion::major_minor(27, 0),
            Some("26A428".to_owned()),
            Arch::Aarch64,
            CompatibilityResult::Pass,
            vec!["ready".to_owned(), "stop".to_owned()],
            vec!["provider_integration_not_covered".to_owned()],
            Some("ready in 493 ms".to_owned()),
            "2026-10-18T11:07:19.973Z".parse().unwrap(),
            "runtime/run/record".to_owned(),
        )
    }

    fn key() -> ProviderRevisionKey {
        ProviderRevisionKey::new(
            "official/postgresql".parse().unwrap(),
            "r1".parse().unwrap(),
            digest(),
        )
    }

    #[test]
    fn a_runtime_record_has_no_provider_revision() {
        assert!(row(CompatibilityScope::Runtime, None).is_ok());
        assert!(row(CompatibilityScope::Runtime, Some(key())).is_err());
    }

    #[test]
    fn a_provider_record_needs_a_provider_revision() {
        assert!(row(CompatibilityScope::Provider, Some(key())).is_ok());
        assert!(row(CompatibilityScope::Provider, None).is_err());
    }

    #[test]
    fn a_record_rejects_a_bad_code_build_or_evidence() {
        let ok = row(CompatibilityScope::Runtime, None).unwrap();
        let build = |edit: &dyn Fn(&mut CompatibilityRecordRow)| {
            let mut next = ok.clone();
            edit(&mut next);
            CompatibilityRecordRow::new(
                next.scope,
                next.provider_revision,
                next.os,
                next.os_version,
                next.os_build,
                next.arch,
                next.result,
                next.checks,
                next.known_limitations,
                next.summary,
                next.tested_at,
                next.evidence,
            )
        };
        assert!(build(&|r| r.checks = vec!["Bad Code".to_owned()]).is_err());
        assert!(build(&|r| r.checks = vec![String::new()]).is_err());
        assert!(build(&|r| r.checks = vec!["a".to_owned(); CODES_MAX + 1]).is_err());
        assert!(build(&|r| r.os_build = Some("26 A".to_owned())).is_err());
        assert!(build(&|r| r.summary = Some(String::new())).is_err());
        assert!(build(&|r| r.evidence = "runtime/../x".to_owned()).is_err());
        assert!(build(&|r| r.evidence = "noslash".to_owned()).is_err());
    }

    #[test]
    fn json_wrappers_check_only_the_outer_shape() {
        assert!(SignatureInfoJson::new("{}".to_owned()).is_ok());
        assert!(SignatureInfoJson::new("{".to_owned()).is_err());
        assert!(DependencyManifestJson::new("[]".to_owned()).is_err());
        assert_eq!(
            "{\"a\":1}"
                .parse::<DependencyManifestJson>()
                .unwrap()
                .as_str(),
            "{\"a\":1}"
        );
    }
}
