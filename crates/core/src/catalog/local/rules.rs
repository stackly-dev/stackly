//! The rules a local package must pass to be imported (spec 0037, *Aturan
//! import*), the verdict over a `check_package` result, and the import
//! limits, which depend on the registry rows.

use std::path::Path;

use stackly_domain::{ProviderImportRecord, ProviderName};
use stackly_platform::HttpsUrl;

use super::digest::{package_bytes, package_sha256};
use super::permissions::{Permissions, permissions};
use crate::catalog::model::{Catalog, CatalogOptions};
use crate::catalog::rules::{OFFICIAL_NAMESPACE, TESTING_NAMESPACE, package_limits};
use crate::provider::{
    ArtifactStatus, Category, Finding, FindingOrigin, Issue, IssueCode, Location, PackageCheck,
    Severity, ValidatedPackage,
};

/// The namespace Stackly keeps for itself, never importable.
pub const STACKLY_NAMESPACE: &str = "stackly";
/// Most imported local providers.
pub const LOCAL_PROVIDERS_MAX: usize = 32;
/// Most bytes of every imported package together.
pub const LOCAL_BYTES_MAX: u64 = 16 * 1024 * 1024;
/// The longest import path, in bytes of UTF-8.
pub const IMPORT_PATH_MAX: usize = 1024;

/// Checks the rules of this spec on a validated package, the ones that do
/// not depend on the registry: the reserved namespaces, the `testing`
/// category, `https` artifact URLs, `fixture` artifacts, and the per
/// provider limits of spec 0017. Pure.
///
/// `init_port_issues` is not here: it comes as an `authoring` finding of
/// `check_package`. With `allow_testing_namespace`, which only test code
/// sets, the `testing` namespace, the `testing` category, and `fixture`
/// artifacts are accepted.
#[must_use]
pub fn check_importable(package: &ValidatedPackage, options: CatalogOptions) -> Vec<Issue> {
    let mut issues = Vec::new();
    let id = package.key().provider();
    let reserved = match id.namespace() {
        OFFICIAL_NAMESPACE | STACKLY_NAMESPACE => true,
        TESTING_NAMESPACE => !options.allow_testing_namespace,
        _ => false,
    };
    if reserved {
        issues.push(Issue::new(
            "provider.yaml",
            Location::Field("metadata.id".into()),
            IssueCode::NamespaceNotAllowed,
            format!("namespace {} can't be imported", id.namespace()),
        ));
    }
    if package.manifest().category() == Category::Testing && !options.allow_testing_namespace {
        issues.push(Issue::new(
            "provider.yaml",
            Location::Field("metadata.category".into()),
            IssueCode::InvalidValue,
            "category testing can't be imported",
        ));
    }
    for (index, artifact) in package.versions().artifacts().iter().enumerate() {
        match &artifact.status {
            ArtifactStatus::Available { urls, .. } => {
                for (position, url) in urls.iter().enumerate() {
                    if let Err(error) = HttpsUrl::new(url) {
                        issues.push(Issue::new(
                            "versions.json",
                            Location::Field(format!("artifacts[{index}].urls[{position}]")),
                            IssueCode::VersionsRule,
                            format!("the url is not an https url: {error}"),
                        ));
                    }
                }
            }
            ArtifactStatus::Fixture { .. } if !options.allow_testing_namespace => {
                issues.push(Issue::new(
                    "versions.json",
                    Location::Field(format!("artifacts[{index}].status")),
                    IssueCode::VersionsRule,
                    "a fixture artifact can't be imported",
                ));
            }
            _ => {}
        }
    }
    for violation in package_limits(package) {
        issues.push(
            Issue::new(
                violation.file,
                Location::Field(violation.location),
                IssueCode::CatalogLimit,
                violation.message,
            )
            .with_reason(violation.kind.as_code()),
        );
    }
    issues
}

/// With the test option, an import may not take the ID of a catalog entry
/// (spec 0037, *Aturan import*): the issue for that, if `id` is one.
#[must_use]
pub fn catalog_id_issue(catalog: &Catalog, id: &ProviderName) -> Option<Issue> {
    catalog
        .entries()
        .iter()
        .any(|entry| entry.id() == id)
        .then(|| {
            Issue::new(
                "provider.yaml",
                Location::Field("metadata.id".into()),
                IssueCode::NamespaceNotAllowed,
                format!("{id} is a catalog provider and can't be imported"),
            )
        })
}

/// Why a package cannot be imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImportBlockReason {
    /// A finding of origin `package` or `authoring` is an error, or an
    /// import rule fails.
    PackageInvalid,
    /// An import limit is reached.
    ImportLimit,
}

impl ImportBlockReason {
    /// Returns the stable code, such as `import_limit`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::PackageInvalid => "package_invalid",
            Self::ImportLimit => "import_limit",
        }
    }
}

/// What the import rules decide about one package folder: the findings of
/// `check_package` an import uses, the import issues, and, when the
/// package validated, its digests and permissions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ImportCheck {
    /// The `check_package` result, with the `scenarios` findings dropped:
    /// an import never copies or reads `tests/`.
    pub check: PackageCheck,
    /// The issues of [`check_importable`] (and [`catalog_id_issue`]).
    pub import_issues: Vec<Issue>,
    /// The package digest, when the package validated.
    pub package_sha256: Option<stackly_domain::Sha256Digest>,
    /// The package size in bytes, when the package validated.
    pub package_bytes: u64,
    /// The permissions, when the package validated.
    pub permissions: Option<Permissions>,
}

impl ImportCheck {
    /// Applies the import rules to a `check_package` result (spec 0037,
    /// *Kontrak dari spec 0036*): `scenarios` findings are dropped, and
    /// when the package validated, [`check_importable`] runs and, with the
    /// test option, the ID must not be one of `catalog`.
    #[must_use]
    pub fn new(
        mut check: PackageCheck,
        options: CatalogOptions,
        catalog: Option<&Catalog>,
    ) -> Self {
        check
            .findings
            .retain(|finding| finding.origin != FindingOrigin::Scenarios);
        check.scenarios.clear();
        let mut import_issues = Vec::new();
        let (package_sha256, bytes, permissions) = match &check.package {
            Some(package) => {
                import_issues = check_importable(package, options);
                if let Some(catalog) = catalog
                    && let Some(issue) = catalog_id_issue(catalog, package.key().provider())
                {
                    import_issues.push(issue);
                }
                (
                    Some(package_sha256(package.files())),
                    package_bytes(package.files()),
                    Some(permissions(package)),
                )
            }
            None => (None, 0, None),
        };
        Self {
            check,
            import_issues,
            package_sha256,
            package_bytes: bytes,
            permissions,
        }
    }

    /// Returns the findings an import shows: `package` and `authoring`, in
    /// their order.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.check.findings
    }

    /// Returns whether the package passes every rule that does not depend
    /// on the registry: it validated, no `package` or `authoring` finding
    /// is an error, and no import issue holds. A `warning` never blocks.
    #[must_use]
    pub fn valid(&self) -> bool {
        self.check.package.is_some()
            && self.import_issues.is_empty()
            && !self
                .check
                .findings
                .iter()
                .any(|finding| finding.severity == Severity::Error)
    }

    /// Returns the validated package of a valid check.
    #[must_use]
    pub fn package(&self) -> Option<&ValidatedPackage> {
        self.check.package.as_ref()
    }
}

/// Which import limit a new import would break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImportLimit {
    /// More than [`LOCAL_PROVIDERS_MAX`] imported providers.
    Count,
    /// More than [`LOCAL_BYTES_MAX`] bytes of imported packages together.
    Bytes,
}

impl ImportLimit {
    /// Returns the `limit` detail of `provider.import_limit`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Bytes => "bytes",
        }
    }

    /// Returns the `max` detail of `provider.import_limit`.
    #[must_use]
    pub const fn max(self) -> u64 {
        match self {
            Self::Count => LOCAL_PROVIDERS_MAX as u64,
            Self::Bytes => LOCAL_BYTES_MAX,
        }
    }
}

/// Decides whether importing `package_bytes` bytes as `provider` stays
/// within the import limits, given every stored import row. An import that
/// replaces the row of `provider` counts only the difference. Pure: the
/// preview calls it on what it read, and the commit guard again inside the
/// write transaction.
#[must_use]
pub fn import_limit(
    existing: &[ProviderImportRecord],
    provider: &ProviderName,
    package_bytes: u64,
) -> Option<ImportLimit> {
    let others = existing
        .iter()
        .filter(|record| &record.provider != provider);
    let (count, bytes) = others.fold((0_usize, 0_u64), |(count, bytes), record| {
        (count + 1, bytes.saturating_add(record.package_bytes))
    });
    if count + 1 > LOCAL_PROVIDERS_MAX {
        Some(ImportLimit::Count)
    } else if bytes.saturating_add(package_bytes) > LOCAL_BYTES_MAX {
        Some(ImportLimit::Bytes)
    } else {
        None
    }
}

/// Why an import path is refused before anything is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ImportPathError {
    /// The path is not absolute.
    #[error("the path is not absolute")]
    NotAbsolute,
    /// The path is not UTF-8.
    #[error("the path is not UTF-8")]
    NotUtf8,
    /// The path is longer than [`IMPORT_PATH_MAX`] bytes.
    #[error("the path is longer than 1024 bytes")]
    TooLong,
    /// The path holds a `.` or `..` component; it is refused, not
    /// normalized.
    #[error("the path holds a . or .. component")]
    NotNormalized,
}

/// Checks an import path (spec 0037 AC-2): absolute, UTF-8, at most
/// [`IMPORT_PATH_MAX`] bytes, and without a `.` or `..` component. Whether
/// it names a folder that is not a symlink is for the reader to find out.
///
/// # Errors
///
/// Returns the first [`ImportPathError`] the path breaks.
pub fn check_import_path(path: &Path) -> Result<&str, ImportPathError> {
    let text = path.to_str().ok_or(ImportPathError::NotUtf8)?;
    if !text.starts_with('/') {
        return Err(ImportPathError::NotAbsolute);
    }
    if text.len() > IMPORT_PATH_MAX {
        return Err(ImportPathError::TooLong);
    }
    if text
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(ImportPathError::NotNormalized);
    }
    Ok(text)
}
