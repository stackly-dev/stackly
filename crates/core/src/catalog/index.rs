//! `catalog.json`, schema `stackly.catalog/v1` (spec 0017, *Bentuk catalog
//! terpasang*): the index `cargo xtask catalog build` writes and the daemon
//! reads, and the content digest that ties it to its revision.

use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stackly_domain::Os;

use crate::provider::{PackageFiles, ValidatedPackage};

use super::rules::TileColor;

/// The only `schema` of `catalog.json` this build reads and writes.
pub const CATALOG_SCHEMA: &str = "stackly.catalog/v1";

/// The file name of the index in a catalog folder.
pub const CATALOG_INDEX_FILE: &str = "catalog.json";

/// The folder of the package copies in a catalog folder.
pub const CATALOG_PACKAGES_DIR: &str = "packages";

/// The whole `catalog.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CatalogIndex {
    /// Always [`CATALOG_SCHEMA`].
    pub schema: String,
    /// The catalog revision, from `providers/catalog.toml`.
    pub revision: u64,
    /// When the maintainers published this revision, in UTC.
    pub published_at: Timestamp,
    /// The [`content_sha256`] of `providers` and `planned`.
    pub content_sha256: String,
    /// Provider entries, sorted by ID.
    pub providers: Vec<IndexProvider>,
    /// Planned entries, sorted by ID.
    pub planned: Vec<IndexPlanned>,
}

/// One provider entry of the index: a package copy and its digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IndexProvider {
    /// The provider ID, such as `official/nats`; its package lives in
    /// `packages/<namespace>/<name>`.
    pub id: String,
    /// The `manifest_sha256` of the package (spec 0009).
    pub manifest_sha256: String,
    /// The SHA-256 of the bytes of its `versions.json`.
    pub versions_sha256: String,
    /// The one or two characters of the tile.
    pub mark: String,
    /// The system color of the tile.
    pub tile: TileColor,
    /// Notes for operating systems the package has no artifact for.
    pub platform_notes: Vec<IndexPlatformNote>,
}

/// A planned entry: a provider on the roadmap without a package yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IndexPlanned {
    /// The provider ID it will have.
    pub id: String,
    /// The name shown on its card.
    pub display_name: String,
    /// Its category code, such as `database`.
    pub category: String,
    /// What it is, for its card.
    pub description: String,
    /// An optional note shown in its detail panel.
    pub note: Option<String>,
    /// The one or two characters of the tile.
    pub mark: String,
    /// The system color of the tile.
    pub tile: TileColor,
}

/// What the catalog says about one operating system a provider has no
/// artifact for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IndexPlatformNote {
    /// The operating system, `linux` or `windows`.
    pub os: Os,
    /// `planned` or `unsupported`.
    pub status: String,
    /// The note, 1 to 256 bytes without control characters.
    pub note: String,
}

/// The part of the index the content digest covers, in field order.
#[derive(Serialize)]
struct CatalogContent<'a> {
    providers: &'a [IndexProvider],
    planned: &'a [IndexPlanned],
}

/// Returns the content digest of a catalog: the SHA-256, as lowercase hex,
/// of the compact `serde_json` form of `{ providers, planned }` with each
/// list sorted by ID. `revision`, `publishedAt`, and `contentSha256` are
/// not part of it.
#[must_use]
pub fn content_sha256(providers: &[IndexProvider], planned: &[IndexPlanned]) -> String {
    let content = CatalogContent { providers, planned };
    // Serializing plain strings, numbers, and lists to a Vec cannot fail.
    let bytes = serde_json::to_vec(&content).unwrap_or_default();
    hex(&Sha256::digest(bytes))
}

/// Returns the SHA-256 of the `versions.json` bytes of a package, as
/// lowercase hex, or `None` when the files hold no `versions.json`.
#[must_use]
pub fn versions_sha256(files: &PackageFiles) -> Option<String> {
    files
        .get("versions.json")
        .map(|bytes| hex(&Sha256::digest(bytes)))
}

/// Builds the index entry of a validated package.
#[must_use]
pub fn provider_entry(
    package: &ValidatedPackage,
    mark: String,
    tile: TileColor,
    platform_notes: Vec<IndexPlatformNote>,
) -> IndexProvider {
    IndexProvider {
        id: package.key().provider().to_string(),
        manifest_sha256: package.key().manifest_sha256().to_string(),
        versions_sha256: versions_sha256(package.files()).unwrap_or_default(),
        mark,
        tile,
        platform_notes,
    }
}

/// Returns the folder of a provider's package copy inside a catalog
/// folder, relative to it: `packages/<namespace>/<name>`.
#[must_use]
pub fn package_dir(id: &str) -> String {
    format!("{CATALOG_PACKAGES_DIR}/{id}")
}

impl CatalogIndex {
    /// Builds an index with its content digest, sorting both lists by ID.
    #[must_use]
    pub fn new(
        revision: u64,
        published_at: Timestamp,
        mut providers: Vec<IndexProvider>,
        mut planned: Vec<IndexPlanned>,
    ) -> Self {
        providers.sort_by(|a, b| a.id.cmp(&b.id));
        planned.sort_by(|a, b| a.id.cmp(&b.id));
        let content_sha256 = content_sha256(&providers, &planned);
        Self {
            schema: CATALOG_SCHEMA.to_owned(),
            revision,
            published_at,
            content_sha256,
            providers,
            planned,
        }
    }

    /// Writes the index as pretty JSON with an indent of 2 spaces and one
    /// newline at the end, the bytes `catalog build` commits to a folder.
    #[must_use]
    pub fn to_pretty_json(&self) -> String {
        // The index holds only strings, numbers, and lists, so this cannot
        // fail.
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Reads an index strictly: the `schema` first, then every field, with
    /// unknown fields rejected.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::UnsupportedSchema`] for another `schema` and
    /// [`IndexError::Invalid`] for anything that does not decode.
    pub fn from_json(bytes: &[u8]) -> Result<Self, IndexError> {
        #[derive(Deserialize)]
        struct SchemaOnly {
            schema: Option<serde_json::Value>,
        }
        let probe: SchemaOnly =
            serde_json::from_slice(bytes).map_err(|err| IndexError::Invalid(err.to_string()))?;
        match probe.schema {
            Some(serde_json::Value::String(schema)) if schema == CATALOG_SCHEMA => {}
            Some(serde_json::Value::String(schema)) => {
                return Err(IndexError::UnsupportedSchema(schema));
            }
            _ => return Err(IndexError::Invalid("schema is missing or not text".into())),
        }
        serde_json::from_slice(bytes).map_err(|err| IndexError::Invalid(err.to_string()))
    }
}

/// Why `catalog.json` could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IndexError {
    /// The `schema` is not [`CATALOG_SCHEMA`].
    #[error("catalog schema {0} is not supported")]
    UnsupportedSchema(String),
    /// The text does not decode as the index.
    #[error("catalog index is invalid: {0}")]
    Invalid(String),
}

/// Lowercase hex of a digest.
fn hex(bytes: &[u8]) -> String {
    struct Hex<'a>(&'a [u8]);
    impl fmt::Display for Hex<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
        }
    }
    Hex(bytes).to_string()
}

#[cfg(test)]
mod tests {
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

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn the_index_sorts_its_lists_and_carries_their_digest() {
        let index = CatalogIndex::new(
            3,
            at("2026-10-19T14:05:00Z"),
            Vec::new(),
            vec![planned("official/mysql"), planned("official/clickhouse")],
        );

        assert_eq!(index.planned[0].id, "official/clickhouse");
        assert_eq!(
            index.content_sha256,
            content_sha256(&index.providers, &index.planned)
        );
        assert_eq!(index.content_sha256.len(), 64);
    }

    #[test]
    fn the_digest_ignores_revision_and_time_but_follows_the_content() {
        let a = CatalogIndex::new(
            1,
            at("2026-10-19T00:00:00Z"),
            vec![],
            vec![planned("official/mysql")],
        );
        let b = CatalogIndex::new(
            9,
            at("2027-01-01T00:00:00Z"),
            vec![],
            vec![planned("official/mysql")],
        );
        let mut changed = planned("official/mysql");
        changed.description = "Relational server!".into();
        let c = CatalogIndex::new(1, at("2026-10-19T00:00:00Z"), vec![], vec![changed]);

        assert_eq!(a.content_sha256, b.content_sha256);
        assert_ne!(a.content_sha256, c.content_sha256);
    }

    #[test]
    fn the_index_round_trips_through_its_pretty_json() {
        let index = CatalogIndex::new(
            1,
            at("2026-10-19T14:05:00Z"),
            vec![],
            vec![planned("official/mysql")],
        );
        let text = index.to_pretty_json();

        assert!(text.ends_with("}\n"));
        assert!(text.contains("\"publishedAt\": \"2026-10-19T14:05:00Z\""));
        assert_eq!(CatalogIndex::from_json(text.as_bytes()).unwrap(), index);
    }

    #[test]
    fn another_schema_or_an_unknown_field_is_rejected() {
        let index = CatalogIndex::new(1, at("2026-10-19T14:05:00Z"), vec![], vec![]);
        let text = index.to_pretty_json();

        let v2 = text.replace("stackly.catalog/v1", "stackly.catalog/v2");
        assert_eq!(
            CatalogIndex::from_json(v2.as_bytes()),
            Err(IndexError::UnsupportedSchema("stackly.catalog/v2".into()))
        );
        let extra = text.replacen("{", "{\n  \"path\": \"../x\",", 1);
        assert!(matches!(
            CatalogIndex::from_json(extra.as_bytes()),
            Err(IndexError::Invalid(_))
        ));
        assert!(matches!(
            CatalogIndex::from_json(b"{}"),
            Err(IndexError::Invalid(_))
        ));
        assert!(matches!(
            CatalogIndex::from_json(b"not json"),
            Err(IndexError::Invalid(_))
        ));
    }

    #[test]
    fn a_package_dir_is_derived_from_the_id() {
        assert_eq!(package_dir("official/nats"), "packages/official/nats");
    }
}
