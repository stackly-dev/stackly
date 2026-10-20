//! Limits and rules of the catalog that `cargo xtask catalog build` and the
//! daemon loader share (spec 0017, *Batas catalog*), plus the tile hints.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use stackly_domain::{Os, ProviderName};

use crate::provider::{ArtifactStatus, Category, ValidatedPackage};

use super::index::{CatalogIndex, IndexPlatformNote};

/// Most bytes of `catalog.json`.
pub const INDEX_MAX_LEN: usize = 256 * 1024;
/// Most provider entries.
pub const PROVIDERS_MAX: usize = 64;
/// Most planned entries.
pub const PLANNED_MAX: usize = 64;
/// Most artifacts in one provider's `versions.json`.
pub const ARTIFACTS_MAX: usize = 32;
/// Most dependencies of one artifact, once `versions.json` carries them
/// (feature 18).
pub const DEPENDENCIES_MAX: usize = 24;
/// Most platform notes of one provider, one per `linux` and `windows`.
pub const PLATFORM_NOTES_MAX: usize = 2;
/// Most bytes of a platform or planned note.
pub const NOTE_MAX_LEN: usize = 256;
/// Most bytes of a blocked reason.
pub const BLOCKED_REASON_MAX_LEN: usize = 256;
/// Most bytes of an artifact ID.
pub const ARTIFACT_ID_MAX_LEN: usize = 128;
/// Most bytes of a license.
pub const LICENSE_MAX_LEN: usize = 64;
/// Most bytes of a license URL.
pub const LICENSE_URL_MAX_LEN: usize = 256;
/// Most bytes of every package file of a catalog together.
pub const PACKAGE_BYTES_MAX: usize = 16 * 1024 * 1024;
/// Most exact versions a provider summary lists.
pub const SUMMARY_VERSIONS_MAX: usize = 16;
/// Most characters of a planned display name, the manifest rule.
const DISPLAY_NAME_MAX: usize = 64;
/// Most characters of a planned description, the manifest rule.
const DESCRIPTION_MAX: usize = 512;

/// The namespace of the official catalog.
pub const OFFICIAL_NAMESPACE: &str = "official";
/// The namespace of test packages, never in the app catalog.
pub const TESTING_NAMESPACE: &str = "testing";

/// The system color of a provider tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TileColor {
    /// System blue.
    Blue,
    /// System cyan.
    Cyan,
    /// System teal.
    Teal,
    /// System green.
    Green,
    /// System yellow.
    Yellow,
    /// System orange.
    Orange,
    /// System red.
    Red,
    /// System brown.
    Brown,
    /// System indigo.
    Indigo,
    /// System gray, the default.
    #[default]
    Gray,
}

impl TileColor {
    /// Every color, in code order.
    pub const ALL: [Self; 10] = [
        Self::Blue,
        Self::Cyan,
        Self::Teal,
        Self::Green,
        Self::Yellow,
        Self::Orange,
        Self::Red,
        Self::Brown,
        Self::Indigo,
        Self::Gray,
    ];

    /// Returns the stable code, such as `blue`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Cyan => "cyan",
            Self::Teal => "teal",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Orange => "orange",
            Self::Red => "red",
            Self::Brown => "brown",
            Self::Indigo => "indigo",
            Self::Gray => "gray",
        }
    }

    /// Reads a color from its code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.as_code() == code)
    }
}

impl fmt::Display for TileColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Returns the stable code of a category, such as `database`.
#[must_use]
pub const fn category_code(category: Category) -> &'static str {
    match category {
        Category::Database => "database",
        Category::Messaging => "messaging",
        Category::Cache => "cache",
        Category::Proxy => "proxy",
        Category::Analytics => "analytics",
        Category::Testing => "testing",
    }
}

/// Reads a category from its code.
#[must_use]
pub fn category_from_code(code: &str) -> Option<Category> {
    Category::from_code(code)
}

/// Returns whether a tile mark is 1 or 2 characters of `[A-Za-z0-9]`.
#[must_use]
pub fn is_mark(mark: &str) -> bool {
    (1..=2).contains(&mark.len()) && mark.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Returns the default mark for a display name: its first two ASCII
/// letters or digits, the first upper case and the second lower case, or
/// `?` when it has none.
#[must_use]
pub fn default_mark(display_name: &str) -> String {
    let mut chars = display_name.chars().filter(char::is_ascii_alphanumeric);
    match (chars.next(), chars.next()) {
        (None, _) => "?".to_owned(),
        (Some(first), None) => first.to_ascii_uppercase().to_string(),
        (Some(first), Some(second)) => {
            format!(
                "{}{}",
                first.to_ascii_uppercase(),
                second.to_ascii_lowercase()
            )
        }
    }
}

/// Checks a platform or planned note: 1 to 256 bytes without control
/// characters.
///
/// # Errors
///
/// Returns the reason when the note breaks the rule.
pub fn check_note(note: &str) -> Result<(), &'static str> {
    if note.is_empty() || note.len() > NOTE_MAX_LEN {
        Err("notes are 1 to 256 bytes")
    } else if note.chars().any(char::is_control) {
        Err("notes have no control characters")
    } else {
        Ok(())
    }
}

/// Returns whether a provider namespace may be in a catalog: `official`,
/// and `testing` only when tests allow it.
#[must_use]
pub fn namespace_allowed(id: &ProviderName, allow_testing: bool) -> bool {
    match id.namespace() {
        OFFICIAL_NAMESPACE => true,
        TESTING_NAMESPACE => allow_testing,
        _ => false,
    }
}

/// Which catalog limit a package breaks, the `reason` of a `catalog_limit`
/// issue (spec 0017, *Issue loader*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LimitKind {
    /// More than [`ARTIFACTS_MAX`] artifacts.
    Artifacts,
    /// More than [`DEPENDENCIES_MAX`] dependencies on one artifact.
    Dependencies,
    /// A text field over its length limit.
    TextLength,
    /// The packages of the catalog over [`PACKAGE_BYTES_MAX`] together.
    TotalBytes,
    /// More than [`PLATFORM_NOTES_MAX`] platform notes.
    PlatformNotes,
}

impl LimitKind {
    /// Returns the stable reason code, such as `artifacts`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Artifacts => "artifacts",
            Self::Dependencies => "dependencies",
            Self::TextLength => "text_length",
            Self::TotalBytes => "total_bytes",
            Self::PlatformNotes => "platform_notes",
        }
    }
}

/// One package that breaks a catalog limit, with where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitViolation {
    /// The file, `versions.json` or `catalog.json`.
    pub file: &'static str,
    /// The field path, such as `artifacts[0].license`.
    pub location: String,
    /// Which limit.
    pub kind: LimitKind,
    /// What is wrong, lowercase without a trailing period.
    pub message: String,
}

/// Returns every catalog limit the `versions.json` of a package breaks.
#[must_use]
pub fn package_limits(package: &ValidatedPackage) -> Vec<LimitViolation> {
    let mut violations = Vec::new();
    let artifacts = package.versions().artifacts();
    if artifacts.len() > ARTIFACTS_MAX {
        violations.push(LimitViolation {
            file: "versions.json",
            location: "artifacts".into(),
            kind: LimitKind::Artifacts,
            message: format!("more than {ARTIFACTS_MAX} artifacts"),
        });
    }
    for (index, artifact) in artifacts.iter().enumerate() {
        let mut text = |field: &str, value: &str, max: usize| {
            if value.len() > max {
                violations.push(LimitViolation {
                    file: "versions.json",
                    location: format!("artifacts[{index}].{field}"),
                    kind: LimitKind::TextLength,
                    message: format!("{field} is longer than {max} bytes"),
                });
            }
        };
        text("id", &artifact.id, ARTIFACT_ID_MAX_LEN);
        match &artifact.status {
            ArtifactStatus::Available {
                license,
                license_url,
                ..
            } => {
                text("license", license, LICENSE_MAX_LEN);
                text("licenseUrl", license_url, LICENSE_URL_MAX_LEN);
            }
            ArtifactStatus::Blocked {
                reason,
                license,
                license_url,
                ..
            } => {
                text("blocked.reason", reason, BLOCKED_REASON_MAX_LEN);
                if let Some(license) = license {
                    text("license", license, LICENSE_MAX_LEN);
                }
                if let Some(license_url) = license_url {
                    text("licenseUrl", license_url, LICENSE_URL_MAX_LEN);
                }
            }
            ArtifactStatus::Fixture { .. } => {}
        }
    }
    violations
}

/// Returns why the platform notes of one provider entry break the rules,
/// or nothing when they follow them.
#[must_use]
pub fn platform_note_problems(notes: &[IndexPlatformNote]) -> Vec<String> {
    let mut problems = Vec::new();
    if notes.len() > PLATFORM_NOTES_MAX {
        problems.push(format!("more than {PLATFORM_NOTES_MAX} platform notes"));
    }
    let mut seen = BTreeSet::new();
    for note in notes {
        if note.os == Os::Macos {
            problems.push("platform notes are only for linux and windows".to_owned());
        }
        if !seen.insert(note.os) {
            problems.push(format!("more than one platform note for {}", note.os));
        }
        if !matches!(note.status.as_str(), "planned" | "unsupported") {
            problems.push("platform note status must be planned or unsupported".to_owned());
        }
        if let Err(reason) = check_note(&note.note) {
            problems.push(format!("platform note: {reason}"));
        }
    }
    problems
}

/// Returns why an index breaks a rule that makes the whole index invalid
/// (spec 0017 AC-3): too many entries, IDs that are not provider names,
/// IDs that repeat or appear in both lists, lists out of ID order, or a
/// planned entry with fields outside their rules. Per entry rules (the
/// namespace, the package, its digests and limits) are the loader's.
#[must_use]
pub fn index_problems(index: &CatalogIndex) -> Vec<String> {
    let mut problems = Vec::new();
    if index.providers.len() > PROVIDERS_MAX {
        problems.push(format!("more than {PROVIDERS_MAX} provider entries"));
    }
    if index.planned.len() > PLANNED_MAX {
        problems.push(format!("more than {PLANNED_MAX} planned entries"));
    }
    let mut ids = BTreeSet::new();
    for (list, entries) in [
        (
            "providers",
            index
                .providers
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "planned",
            index
                .planned
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
        ),
    ] {
        if !entries.windows(2).all(|pair| pair[0] < pair[1]) {
            problems.push(format!("{list} are not sorted by ID without repeats"));
        }
        for id in entries {
            if id.parse::<ProviderName>().is_err() {
                problems.push(format!("{list} entry {id} is not a provider ID"));
            }
            if !ids.insert(id) {
                problems.push(format!("{id} appears more than once"));
            }
        }
    }
    for provider in &index.providers {
        if !is_mark(&provider.mark) {
            problems.push(format!(
                "{} has a mark that is not 1 or 2 letters or digits",
                provider.id
            ));
        }
        for problem in platform_note_problems(&provider.platform_notes) {
            problems.push(format!("{}: {problem}", provider.id));
        }
    }
    for planned in &index.planned {
        let id = &planned.id;
        if planned
            .id
            .parse::<ProviderName>()
            .is_ok_and(|name| name.namespace() != OFFICIAL_NAMESPACE)
        {
            problems.push(format!(
                "planned entry {id} is not in the official namespace"
            ));
        }
        let display_len = planned.display_name.chars().count();
        if display_len == 0
            || display_len > DISPLAY_NAME_MAX
            || planned.display_name.chars().any(char::is_control)
        {
            problems.push(format!("{id} needs a display name of 1 to 64 characters"));
        }
        if !matches!(
            planned.category.as_str(),
            "database" | "messaging" | "cache" | "proxy" | "analytics"
        ) {
            problems.push(format!(
                "{id} has a category outside the catalog categories"
            ));
        }
        let description_len = planned.description.chars().count();
        if description_len == 0
            || description_len > DESCRIPTION_MAX
            || planned.description.chars().any(char::is_control)
        {
            problems.push(format!("{id} needs a description of 1 to 512 characters"));
        }
        if let Some(note) = &planned.note
            && let Err(reason) = check_note(note)
        {
            problems.push(format!("{id} note: {reason}"));
        }
        if !is_mark(&planned.mark) {
            problems.push(format!(
                "{id} has a mark that is not 1 or 2 letters or digits"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::super::index::{IndexPlanned, IndexProvider};
    use super::*;

    fn planned(id: &str) -> IndexPlanned {
        IndexPlanned {
            id: id.to_owned(),
            display_name: "MySQL".into(),
            category: "database".into(),
            description: "Relational server.".into(),
            note: None,
            mark: "My".into(),
            tile: TileColor::Teal,
        }
    }

    fn provider(id: &str) -> IndexProvider {
        IndexProvider {
            id: id.to_owned(),
            manifest_sha256: "0".repeat(64),
            versions_sha256: "0".repeat(64),
            mark: "Na".into(),
            tile: TileColor::Green,
            platform_notes: Vec::new(),
        }
    }

    fn index(providers: Vec<IndexProvider>, planned: Vec<IndexPlanned>) -> CatalogIndex {
        CatalogIndex {
            schema: super::super::index::CATALOG_SCHEMA.to_owned(),
            revision: 1,
            published_at: Timestamp::UNIX_EPOCH,
            content_sha256: String::new(),
            providers,
            planned,
        }
    }

    fn note(os: Os, status: &str, text: &str) -> IndexPlatformNote {
        IndexPlatformNote {
            os,
            status: status.into(),
            note: text.into(),
        }
    }

    #[test]
    fn default_marks_take_the_first_two_ascii_letters_or_digits() {
        assert_eq!(default_mark("PostgreSQL"), "Po");
        assert_eq!(default_mark("NATS Server"), "Na");
        assert_eq!(default_mark("x"), "X");
        assert_eq!(default_mark("Ünï 9"), "N9");
        assert_eq!(default_mark("ä ö"), "?");
    }

    #[test]
    fn marks_are_one_or_two_ascii_letters_or_digits() {
        assert!(is_mark("Pg"));
        assert!(is_mark("9"));
        assert!(!is_mark(""));
        assert!(!is_mark("Pgx"));
        assert!(!is_mark("P-"));
        assert!(!is_mark("Ü"));
    }

    #[test]
    fn tile_colors_round_trip_through_their_codes() {
        for color in TileColor::ALL {
            assert_eq!(TileColor::from_code(color.as_code()), Some(color));
        }
        assert_eq!(TileColor::from_code("purple"), None);
        assert_eq!(TileColor::default(), TileColor::Gray);
    }

    #[test]
    fn only_official_and_allowed_testing_namespaces_are_accepted() {
        let official: ProviderName = "official/nats".parse().unwrap();
        let testing: ProviderName = "testing/fake".parse().unwrap();
        let community: ProviderName = "community/x".parse().unwrap();

        assert!(namespace_allowed(&official, false));
        assert!(!namespace_allowed(&testing, false));
        assert!(namespace_allowed(&testing, true));
        assert!(!namespace_allowed(&community, true));
    }

    #[test]
    fn notes_have_a_length_and_no_control_characters() {
        assert!(check_note("Windows releases exist upstream.").is_ok());
        assert!(check_note("").is_err());
        assert!(check_note(&"a".repeat(257)).is_err());
        assert!(check_note("line\nbreak").is_err());
    }

    #[test]
    fn a_well_formed_index_has_no_problems() {
        assert_eq!(
            index_problems(&index(
                vec![provider("official/nats")],
                vec![planned("official/mysql"), planned("official/valkey")]
            )),
            Vec::<String>::new()
        );
    }

    #[test]
    fn repeated_unsorted_overlapping_or_malformed_ids_break_the_index() {
        let repeated = index(
            vec![],
            vec![planned("official/mysql"), planned("official/mysql")],
        );
        let unsorted = index(
            vec![],
            vec![planned("official/valkey"), planned("official/mysql")],
        );
        let overlap = index(
            vec![provider("official/mysql")],
            vec![planned("official/mysql")],
        );
        let malformed = index(vec![provider("Official/NATS")], vec![]);

        for broken in [repeated, unsorted, overlap, malformed] {
            assert!(!index_problems(&broken).is_empty(), "{broken:?}");
        }
    }

    #[test]
    fn too_many_entries_break_the_index() {
        let planned: Vec<IndexPlanned> = (0..65)
            .map(|n| planned(&format!("official/p{n:03}")))
            .collect();

        assert_eq!(
            index_problems(&index(vec![], planned)),
            ["more than 64 planned entries"]
        );
    }

    #[test]
    fn planned_fields_follow_the_manifest_rules() {
        let mut bad = planned("official/mysql");
        bad.category = "testing".into();
        bad.description = String::new();
        bad.mark = "MyS".into();
        let mut community = planned("community/mysql");
        community.mark = "My".into();

        assert_eq!(index_problems(&index(vec![], vec![bad])).len(), 3);
        assert_eq!(index_problems(&index(vec![], vec![community])).len(), 1);
    }

    #[test]
    fn platform_notes_are_one_per_linux_or_windows_with_a_closed_status() {
        assert_eq!(
            platform_note_problems(&[note(Os::Windows, "planned", "Pending tests.")]),
            Vec::<String>::new()
        );
        assert_eq!(
            platform_note_problems(&[
                note(Os::Macos, "planned", "x"),
                note(Os::Linux, "supported", "x"),
                note(Os::Linux, "planned", ""),
            ])
            .len(),
            5
        );
    }
}
