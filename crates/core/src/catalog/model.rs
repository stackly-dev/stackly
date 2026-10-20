//! The catalog read model the daemon keeps in memory (spec 0017, *Read
//! model di daemon*): entries in display order and the facts about the
//! catalog itself.

use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{ExactVersion, Os, ProviderName, Sha256Digest, SupportStatus};

use crate::provider::{Category, Issue, ValidatedPackage, VersionArtifact};

use super::rules::TileColor;

/// How old a catalog may be before it is outdated: 180 days.
pub const CATALOG_OUTDATED_AFTER: SignedDuration = SignedDuration::from_hours(180 * 24);

/// Where a catalog folder came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogLocation {
    /// `Contents/Resources/catalog` of the app bundle, canonical.
    Bundled(PathBuf),
    /// `STACKLY_CATALOG_DIR`, for development and tests, canonical.
    Development(PathBuf),
}

impl CatalogLocation {
    /// Returns the catalog folder.
    #[must_use]
    pub const fn path(&self) -> &PathBuf {
        match self {
            Self::Bundled(path) | Self::Development(path) => path,
        }
    }
}

/// Options the daemon passes to the loader. Only test code changes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CatalogOptions {
    /// Accept packages of the `testing` namespace. Never set from an
    /// environment variable or a flag.
    pub allow_testing_namespace: bool,
}

/// Why a catalog could not be loaded at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnavailableReason {
    /// The folder has no `catalog.json`.
    Missing,
    /// `catalog.json` is unreadable, too large, does not decode, or breaks
    /// an index rule.
    IndexInvalid,
    /// `catalog.json` has another `schema`.
    UnsupportedSchema,
}

impl UnavailableReason {
    /// Returns the stable code, the `reason` detail of
    /// `catalog.unavailable`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::IndexInvalid => "index_invalid",
            Self::UnsupportedSchema => "unsupported_schema",
        }
    }
}

/// The catalog the daemon holds for one run.
#[derive(Debug, Clone)]
pub enum CatalogState {
    /// No `STACKLY_CATALOG_DIR` and no app bundle.
    NotConfigured,
    /// A catalog folder exists but cannot be used.
    Unavailable {
        /// Why.
        reason: UnavailableReason,
        /// One safe English sentence for the health detail.
        detail: String,
    },
    /// The loaded catalog.
    Loaded(Arc<Catalog>),
}

/// The tile of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileHint {
    /// One or two characters.
    pub mark: String,
    /// The system color.
    pub tile: TileColor,
}

/// What the catalog says about one OS a provider has no artifact for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformNote {
    /// `linux` or `windows`.
    pub os: Os,
    /// `planned` or `unsupported`.
    pub status: SupportStatus,
    /// The note.
    pub note: String,
}

/// A provider with a validated package.
#[derive(Debug, Clone)]
pub struct ProviderEntry {
    /// The package.
    pub package: Arc<ValidatedPackage>,
    /// Its tile.
    pub tile: TileHint,
    /// Its platform notes, by OS.
    pub notes: Vec<PlatformNote>,
}

impl ProviderEntry {
    /// Returns the artifacts of its `versions.json`, in file order.
    #[must_use]
    pub fn artifacts(&self) -> &[VersionArtifact] {
        self.package.versions().artifacts()
    }

    /// Returns every exact version once, in the order it first appears in
    /// `versions.json`.
    #[must_use]
    pub fn versions(&self) -> Vec<&ExactVersion> {
        let mut versions: Vec<&ExactVersion> = Vec::new();
        for artifact in self.artifacts() {
            if !versions.contains(&&artifact.version) {
                versions.push(&artifact.version);
            }
        }
        versions
    }
}

/// A provider on the roadmap without a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedEntry {
    /// The provider ID it will have.
    pub id: ProviderName,
    /// The name on its card.
    pub display_name: String,
    /// Its category.
    pub category: Category,
    /// What it is.
    pub description: String,
    /// An optional note.
    pub note: Option<String>,
    /// Its tile.
    pub tile: TileHint,
}

/// A provider entry whose package failed a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEntry {
    /// The ID from the index.
    pub id: ProviderName,
    /// The category, when the manifest `metadata` decodes.
    pub category: Option<Category>,
    /// Its tile.
    pub tile: TileHint,
    /// What is wrong, at most `MAX_ISSUES` plus one `too_many_issues`.
    pub issues: Vec<Issue>,
}

/// One entry of the catalog.
#[derive(Debug, Clone)]
pub enum CatalogEntry {
    /// A provider with a validated package.
    Provider(ProviderEntry),
    /// A planned provider.
    Planned(PlannedEntry),
    /// A provider whose package failed a rule.
    Invalid(InvalidEntry),
}

impl CatalogEntry {
    /// Returns the provider ID.
    #[must_use]
    pub fn id(&self) -> &ProviderName {
        match self {
            Self::Provider(entry) => entry.package.key().provider(),
            Self::Planned(entry) => &entry.id,
            Self::Invalid(entry) => &entry.id,
        }
    }

    /// Returns the category, if known.
    #[must_use]
    pub fn category(&self) -> Option<Category> {
        match self {
            Self::Provider(entry) => Some(entry.package.manifest().category()),
            Self::Planned(entry) => Some(entry.category),
            Self::Invalid(entry) => entry.category,
        }
    }

    /// Returns the name shown for it; an invalid entry shows its ID.
    #[must_use]
    pub fn display_name(&self) -> String {
        match self {
            Self::Provider(entry) => entry.package.manifest().display_name().to_owned(),
            Self::Planned(entry) => entry.display_name.clone(),
            Self::Invalid(entry) => entry.id.to_string(),
        }
    }

    /// Returns its tile.
    #[must_use]
    pub const fn tile(&self) -> &TileHint {
        match self {
            Self::Provider(entry) => &entry.tile,
            Self::Planned(entry) => &entry.tile,
            Self::Invalid(entry) => &entry.tile,
        }
    }
}

/// The facts about a loaded catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogMeta {
    /// The catalog revision.
    pub revision: u64,
    /// When it was published.
    pub published_at: Timestamp,
    /// Its content digest, from the index.
    pub content_sha256: Sha256Digest,
    /// Where it was loaded from.
    pub location: CatalogLocation,
    /// When the daemon loaded it.
    pub loaded_at: Timestamp,
}

/// A loaded catalog: its facts and its entries in display order.
#[derive(Debug, Clone)]
pub struct Catalog {
    meta: CatalogMeta,
    entries: Vec<CatalogEntry>,
}

impl Catalog {
    /// Builds a catalog and puts its entries in display order (spec 0017,
    /// *Urutan tampil*).
    pub(crate) fn new(meta: CatalogMeta, mut entries: Vec<CatalogEntry>) -> Self {
        entries.sort_by(display_order);
        Self { meta, entries }
    }

    /// Builds a catalog from validated entries for tests, with fixed test
    /// facts: revision 1, published and loaded at `2026-10-01T00:00:00Z`,
    /// from the development folder `/stackly-test-catalog`. Packages of the
    /// `testing` namespace are kept. `Catalog::for_tests(&[])` is a valid
    /// empty catalog.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn for_tests(entries: &[CatalogEntry]) -> Self {
        let at = Timestamp::constant(1_790_812_800, 0);
        Self::for_tests_with_meta(
            CatalogMeta {
                revision: 1,
                published_at: at,
                content_sha256: test_digest(entries),
                location: CatalogLocation::Development(PathBuf::from("/stackly-test-catalog")),
                loaded_at: at,
            },
            entries,
        )
    }

    /// Builds a catalog from validated entries and the given facts, for
    /// tests.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn for_tests_with_meta(meta: CatalogMeta, entries: &[CatalogEntry]) -> Self {
        Self::new(meta, entries.to_vec())
    }

    /// Returns the facts about the catalog.
    #[must_use]
    pub const fn meta(&self) -> &CatalogMeta {
        &self.meta
    }

    /// Returns every entry in display order.
    #[must_use]
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// Returns the entry with this ID.
    #[must_use]
    pub fn entry(&self, id: &ProviderName) -> Option<&CatalogEntry> {
        self.entries.iter().find(|entry| entry.id() == id)
    }

    /// Returns the IDs whose name part is `name`, for the hint of a
    /// provider asked for without its namespace.
    #[must_use]
    pub fn find_by_name(&self, name: &str) -> Vec<&ProviderName> {
        self.entries
            .iter()
            .map(CatalogEntry::id)
            .filter(|id| id.name() == name)
            .collect()
    }

    /// Returns how many entries failed a rule.
    #[must_use]
    pub fn invalid_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, CatalogEntry::Invalid(_)))
            .count()
    }

    /// Returns whether the catalog was published more than
    /// [`CATALOG_OUTDATED_AFTER`] before `now`.
    #[must_use]
    pub fn outdated(&self, now: Timestamp) -> bool {
        now.duration_since(self.meta.published_at) > CATALOG_OUTDATED_AFTER
    }
}

/// The rank of a category in display order; entries without one come last.
const fn category_rank(category: Option<Category>) -> u8 {
    match category {
        Some(Category::Database) => 0,
        Some(Category::Messaging) => 1,
        Some(Category::Analytics) => 2,
        Some(Category::Cache) => 3,
        Some(Category::Proxy) => 4,
        Some(Category::Testing) => 5,
        None => 6,
    }
}

/// Provider and invalid entries first, planned last; then category, display
/// name, and ID.
fn display_order(a: &CatalogEntry, b: &CatalogEntry) -> Ordering {
    let group = |entry: &CatalogEntry| u8::from(matches!(entry, CatalogEntry::Planned(_)));
    group(a)
        .cmp(&group(b))
        .then_with(|| category_rank(a.category()).cmp(&category_rank(b.category())))
        .then_with(|| a.display_name().cmp(&b.display_name()))
        .then_with(|| a.id().as_str().cmp(b.id().as_str()))
}

/// The content digest of test entries, the same digest a built index of
/// them would carry.
#[cfg(feature = "test-support")]
fn test_digest(entries: &[CatalogEntry]) -> Sha256Digest {
    use super::index::{IndexPlanned, content_sha256, provider_entry};
    use super::rules::category_code;

    let mut providers = Vec::new();
    let mut planned = Vec::new();
    for entry in entries {
        match entry {
            CatalogEntry::Provider(provider) => providers.push(provider_entry(
                &provider.package,
                provider.tile.mark.clone(),
                provider.tile.tile,
                Vec::new(),
            )),
            CatalogEntry::Planned(entry) => planned.push(IndexPlanned {
                id: entry.id.to_string(),
                display_name: entry.display_name.clone(),
                category: category_code(entry.category).to_owned(),
                description: entry.description.clone(),
                note: entry.note.clone(),
                mark: entry.tile.mark.clone(),
                tile: entry.tile.tile,
            }),
            CatalogEntry::Invalid(_) => {}
        }
    }
    providers.sort_by(|a, b| a.id.cmp(&b.id));
    planned.sort_by(|a, b| a.id.cmp(&b.id));
    content_sha256(&providers, &planned)
        .parse()
        .expect("content_sha256 is 64 lowercase hex characters")
}
