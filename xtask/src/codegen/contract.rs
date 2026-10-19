//! The error catalog file, the normalized contract digest, and
//! `protocols/contract-versions.json` (spec 0006, *Versi dan
//! kompatibilitas*).
//!
//! The digest hashes every protocol schema, the catalog, and the contract
//! limits after dropping documentation text and the order of lists whose
//! order means nothing, so a doc comment or a reordered field never changes
//! it. A changed digest needs a new `ProtocolVersion::CURRENT`; `--amend`
//! rewrites the digest of a version that is not on main yet.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use stackly_protocol::ProtocolVersion;
use stackly_protocol::codes::{self, ErrorSpec, ItemSpec};
use stackly_protocol::limits::CONTRACT_LIMITS;

/// The area whose codes only ever appear in CLI output, never on the wire
/// (spec 0008), so adding one never bumps the protocol minor.
const CLI_AREA: &str = "cli";

/// Schema keywords that only document; normalization drops them.
const DOC_KEYWORDS: [&str; 3] = ["description", "title", "examples"];
/// Keywords whose value maps names to schemas.
const SCHEMA_MAP_KEYWORDS: [&str; 5] = [
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
];
/// Keywords whose value is a list whose order means nothing: names, values,
/// or types. Normalization sorts them.
const SET_KEYWORDS: [&str; 3] = ["required", "enum", "type"];
/// Keywords whose value is a list of schemas whose order means nothing:
/// exactly one `oneOf` branch matches and every `allOf` branch must, so
/// normalization sorts them too. `anyOf` keeps its order, because serde
/// tries untagged variants in order, and so does `prefixItems`, whose
/// order is the tuple's.
const SCHEMA_SET_KEYWORDS: [&str; 2] = ["oneOf", "allOf"];
/// Keywords whose value is a schema or a list of schemas.
const SCHEMA_KEYWORDS: [&str; 15] = [
    "allOf",
    "anyOf",
    "oneOf",
    "prefixItems",
    "items",
    "additionalItems",
    "additionalProperties",
    "unevaluatedItems",
    "unevaluatedProperties",
    "not",
    "if",
    "then",
    "else",
    "contains",
    "propertyNames",
];

/// `schema` without documentation keywords, at every schema position, with
/// every list in [`SET_KEYWORDS`] and [`SCHEMA_SET_KEYWORDS`] sorted. A
/// property that happens to be named `title` or `description` stays.
pub fn normalize_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(object) => {
            let mut out = Map::new();
            for (key, value) in object {
                let key_str = key.as_str();
                if DOC_KEYWORDS.contains(&key_str) {
                    continue;
                }
                let normalized = if SCHEMA_MAP_KEYWORDS.contains(&key_str) {
                    match value {
                        Value::Object(named) => Value::Object(
                            named
                                .iter()
                                .map(|(name, sub)| (name.clone(), normalize_schema(sub)))
                                .collect(),
                        ),
                        other => other.clone(),
                    }
                } else if SET_KEYWORDS.contains(&key_str) {
                    sorted(value.clone())
                } else if SCHEMA_SET_KEYWORDS.contains(&key_str) {
                    sorted(normalize_schema(value))
                } else if SCHEMA_KEYWORDS.contains(&key_str) {
                    normalize_schema(value)
                } else {
                    value.clone()
                };
                out.insert(key.clone(), normalized);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize_schema).collect()),
        other => other.clone(),
    }
}

/// `value` with its items sorted by their canonical text when it is a list.
fn sorted(value: Value) -> Value {
    match value {
        Value::Array(items) => {
            let mut keyed: Vec<(String, Value)> = items
                .into_iter()
                .map(|item| {
                    let mut text = String::new();
                    write_canonical(&item, &mut text);
                    (text, item)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Array(keyed.into_iter().map(|(_, item)| item).collect())
        }
        other => other,
    }
}

/// Every catalog entry, sorted by code.
fn sorted_catalog() -> Vec<&'static ErrorSpec> {
    let mut specs: Vec<_> = codes::catalog().collect();
    specs.sort_by_key(|spec| spec.code);
    specs
}

/// `protocols/error-codes.json`: every area and every code, sorted by code,
/// for Swift, the docs, and feature 8.
#[derive(Debug, Serialize)]
pub struct ErrorCodesFile {
    areas: Vec<AreaEntry>,
    codes: Vec<CodeEntry>,
}

#[derive(Debug, Serialize)]
struct AreaEntry {
    name: &'static str,
    owner: &'static str,
}

#[derive(Debug, Serialize)]
struct CodeEntry {
    code: &'static str,
    category: &'static str,
    retryable: bool,
    details: &'static [&'static str],
    items: Option<ItemsEntry>,
    summary: &'static str,
}

#[derive(Debug, Serialize)]
struct ItemsEntry {
    kind: &'static str,
    fields: &'static [&'static str],
    max: u16,
}

impl From<&ItemSpec> for ItemsEntry {
    fn from(items: &ItemSpec) -> Self {
        Self {
            kind: items.kind,
            fields: items.fields,
            max: items.max,
        }
    }
}

/// The contents of `protocols/error-codes.json`.
pub fn error_codes_file() -> ErrorCodesFile {
    ErrorCodesFile {
        areas: codes::AREAS
            .iter()
            .map(|area| AreaEntry {
                name: area.name,
                owner: area.owner,
            })
            .collect(),
        codes: sorted_catalog()
            .into_iter()
            .map(|spec| CodeEntry {
                code: spec.code,
                category: spec.category.as_str(),
                retryable: spec.retryable,
                details: spec.details,
                items: spec.items.as_ref().map(ItemsEntry::from),
                summary: spec.summary,
            })
            .collect(),
    }
}

/// One catalog entry as the digest sees it, so tests can feed entries the
/// real catalog does not have.
#[derive(Debug, Clone, Copy)]
struct DigestEntry {
    code: &'static str,
    category: &'static str,
    retryable: bool,
    details: &'static [&'static str],
    items: Option<(&'static str, &'static [&'static str], u16)>,
}

impl From<&ErrorSpec> for DigestEntry {
    fn from(spec: &ErrorSpec) -> Self {
        Self {
            code: spec.code,
            category: spec.category.as_str(),
            retryable: spec.retryable,
            details: spec.details,
            items: spec
                .items
                .as_ref()
                .map(|items| (items.kind, items.fields, items.max)),
        }
    }
}

/// Whether a code counts toward the digest: every area but `cli`.
fn is_wire_code(code: &str) -> bool {
    code.split_once('.')
        .is_none_or(|(area, _)| area != CLI_AREA)
}

/// The catalog as the digest sees it: code, category, `retryable`, the
/// sorted `details` keys, and the item kind, sorted field keys, and limit,
/// without the summary, and without the `cli` area.
fn normalized_catalog(entries: impl IntoIterator<Item = DigestEntry>) -> Value {
    let mut entries: Vec<DigestEntry> = entries
        .into_iter()
        .filter(|entry| is_wire_code(entry.code))
        .collect();
    entries.sort_by_key(|entry| entry.code);
    let entries: Vec<Value> = entries
        .into_iter()
        .map(|entry| {
            let mut details = entry.details.to_vec();
            details.sort_unstable();
            let items = entry.items.map(|(kind, fields, max)| {
                let mut fields = fields.to_vec();
                fields.sort_unstable();
                json!({ "kind": kind, "fields": fields, "max": max })
            });
            json!({
                "code": entry.code,
                "category": entry.category,
                "retryable": entry.retryable,
                "details": details,
                "items": items,
            })
        })
        .collect();
    Value::Array(entries)
}

/// The real catalog, as [`DigestEntry`] values.
fn catalog_entries() -> impl Iterator<Item = DigestEntry> {
    codes::catalog().map(DigestEntry::from)
}

/// The contract limits as the digest sees them: one object from each
/// camelCase name to its value, so the order of the list never matters.
fn normalized_limits(limits: &[(&str, u64)]) -> Value {
    Value::Object(
        limits
            .iter()
            .map(|(name, value)| ((*name).to_owned(), Value::from(*value)))
            .collect(),
    )
}

/// SHA-256, as lowercase hex, of the normalized schemas (keyed by file
/// stem), the normalized catalog, and the contract limits from
/// `stackly_protocol::limits::CONTRACT_LIMITS`, written as canonical JSON
/// with sorted keys.
pub fn contract_digest(schemas: &[(&str, Value)]) -> String {
    digest_of(schemas, catalog_entries(), CONTRACT_LIMITS)
}

/// [`contract_digest`] over any catalog entries and limits.
fn digest_of(
    schemas: &[(&str, Value)],
    catalog: impl IntoIterator<Item = DigestEntry>,
    limits: &[(&str, u64)],
) -> String {
    let schemas: BTreeMap<&str, Value> = schemas
        .iter()
        .map(|(stem, schema)| (*stem, normalize_schema(schema)))
        .collect();
    let contract = json!({
        "errorCodes": normalized_catalog(catalog),
        "limits": normalized_limits(limits),
        "schemas": schemas,
    });
    let mut canonical = String::new();
    write_canonical(&contract, &mut canonical);
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(canonical.as_bytes()) {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Writes `value` as JSON without whitespace, object keys sorted, so the
/// text never depends on how `serde_json` orders a map.
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// `protocols/contract-versions.json`: the digest of every protocol
/// version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractVersions {
    /// One entry per version, sorted by version.
    pub versions: Vec<VersionEntry>,
}

/// The recorded digest of one protocol version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEntry {
    /// The protocol major.
    pub major: u32,
    /// The protocol minor.
    pub minor: u32,
    /// The normalized contract digest of that version.
    pub digest: String,
}

/// How [`update_versions`] treats an entry for `CURRENT` that disagrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionsMode {
    /// Add a missing entry; replace a disagreeing digest only with `amend`.
    Write { amend: bool },
    /// Change nothing; fail on a missing or disagreeing entry.
    Check,
}

/// The version record after this run, or why it must not change.
pub fn update_versions(
    mut record: ContractVersions,
    current: ProtocolVersion,
    digest: &str,
    mode: VersionsMode,
) -> Result<ContractVersions> {
    let version = format!("{}.{}", current.major, current.minor);
    if let Some(higher) = record
        .versions
        .iter()
        .find(|entry| (entry.major, entry.minor) > (current.major, current.minor))
    {
        bail!(
            "protocols/contract-versions.json records protocol {}.{}, above ProtocolVersion::CURRENT {version}; \
             bump CURRENT in crates/protocol/src/version.rs past it and run `cargo xtask codegen`",
            higher.major,
            higher.minor
        );
    }
    let entry = record
        .versions
        .iter_mut()
        .find(|entry| (entry.major, entry.minor) == (current.major, current.minor));
    match (entry, mode) {
        (Some(entry), _) if entry.digest == digest => {}
        (Some(entry), VersionsMode::Write { amend: true }) => {
            println!("amended the contract digest of protocol {version}");
            digest.clone_into(&mut entry.digest);
        }
        (Some(_), VersionsMode::Write { amend: false }) => bail!(
            "the contract changed but ProtocolVersion::CURRENT is still {version}, whose digest is \
             recorded in protocols/contract-versions.json; bump CURRENT in \
             crates/protocol/src/version.rs (minor for an addition, major for a break), or pass \
             --amend if {version} is not on main yet"
        ),
        (Some(_), VersionsMode::Check) => bail!(
            "the contract digest no longer matches the one recorded for protocol {version}; bump \
             ProtocolVersion::CURRENT and run `cargo xtask codegen`"
        ),
        (None, VersionsMode::Write { .. }) => record.versions.push(VersionEntry {
            major: current.major,
            minor: current.minor,
            digest: digest.to_owned(),
        }),
        (None, VersionsMode::Check) => bail!(
            "protocol {version} has no entry in protocols/contract-versions.json; run `cargo xtask codegen`"
        ),
    }
    record
        .versions
        .sort_by_key(|entry| (entry.major, entry.minor));
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1_0: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };
    const V1_1: ProtocolVersion = ProtocolVersion { major: 1, minor: 1 };

    fn record(entries: &[(u32, u32, &str)]) -> ContractVersions {
        ContractVersions {
            versions: entries
                .iter()
                .map(|&(major, minor, digest)| VersionEntry {
                    major,
                    minor,
                    digest: digest.to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn normalization_drops_documentation_but_keeps_properties_with_those_names() {
        let schema = json!({
            "title": "Thing",
            "description": "doc",
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "a field named title" },
                "items": { "type": "array", "items": { "type": "integer", "examples": [1] } }
            },
            "oneOf": [{ "const": "a", "description": "doc" }]
        });
        assert_eq!(
            normalize_schema(&schema),
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "items": { "type": "array", "items": { "type": "integer" } }
                },
                "oneOf": [{ "const": "a" }]
            })
        );
    }

    #[test]
    fn the_digest_ignores_doc_text_and_key_order_but_not_types() {
        let base = contract_digest(&[("a", json!({ "type": "string", "format": "uuid" }))]);
        let documented = contract_digest(&[(
            "a",
            json!({ "format": "uuid", "description": "now with docs", "type": "string" }),
        )]);
        let changed = contract_digest(&[("a", json!({ "type": "integer" }))]);
        assert_eq!(base, documented);
        assert_ne!(base, changed);
        assert_eq!(base.len(), 64);
        assert!(
            base.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
    }

    /// A small limits list for the tests that feed their own entries.
    const LIMITS: &[(&str, u64)] = &[("controlFrameMax", 1_048_576), ("maxJsonDepth", 32)];

    #[test]
    fn the_digest_sees_every_limit_value_but_not_the_list_order() {
        let schemas = [("a", json!({ "type": "string" }))];
        let base = digest_of(&schemas, [], LIMITS);
        let reordered = digest_of(
            &schemas,
            [],
            &[("maxJsonDepth", 32), ("controlFrameMax", 1_048_576)],
        );
        let lowered = digest_of(
            &schemas,
            [],
            &[("controlFrameMax", 1_048_576), ("maxJsonDepth", 16)],
        );
        let added = digest_of(
            &schemas,
            [],
            &[
                ("controlFrameMax", 1_048_576),
                ("maxJsonDepth", 32),
                ("fieldErrorsMax", 32),
            ],
        );
        assert_eq!(base, reordered);
        assert_ne!(base, lowered);
        assert_ne!(base, added);
    }

    #[test]
    fn the_real_digest_covers_the_protocol_contract_limits() {
        let schemas = [("a", json!({ "type": "string" }))];
        assert_eq!(
            contract_digest(&schemas),
            digest_of(&schemas, catalog_entries(), CONTRACT_LIMITS)
        );
        assert_ne!(
            contract_digest(&schemas),
            digest_of(&schemas, catalog_entries(), LIMITS)
        );
        assert!(
            CONTRACT_LIMITS
                .iter()
                .any(|(name, _)| *name == "subscribeKindsMax")
        );
    }

    #[test]
    fn normalization_sorts_unordered_lists_but_keeps_ordered_ones() {
        let schema = |required: [&str; 2], one_of: [&str; 2], any_of: [&str; 2]| {
            json!({
                "type": ["object", "null"],
                "required": required,
                "properties": { "kind": { "enum": one_of } },
                "oneOf": [{ "const": one_of[0] }, { "const": one_of[1] }],
                "allOf": [{ "const": one_of[0] }, { "const": one_of[1] }],
                "anyOf": [{ "const": any_of[0] }, { "const": any_of[1] }],
                "prefixItems": [{ "const": any_of[0] }, { "const": any_of[1] }],
            })
        };
        let base = normalize_schema(&schema(["a", "b"], ["x", "y"], ["p", "q"]));
        let reordered = normalize_schema(&schema(["b", "a"], ["y", "x"], ["p", "q"]));
        let any_reordered = normalize_schema(&schema(["a", "b"], ["x", "y"], ["q", "p"]));
        assert_eq!(base, reordered);
        assert_ne!(base, any_reordered);
        assert_eq!(base["required"], json!(["a", "b"]));
        assert_eq!(base["properties"]["kind"]["enum"], json!(["x", "y"]));
        let mut types = schema(["a", "b"], ["x", "y"], ["p", "q"]);
        types["type"] = json!(["null", "object"]);
        assert_eq!(normalize_schema(&types), base);
    }

    fn entry(code: &'static str) -> DigestEntry {
        DigestEntry {
            code,
            category: "invalid_input",
            retryable: false,
            details: &["path"],
            items: None,
        }
    }

    #[test]
    fn cli_codes_never_change_the_digest_but_wire_codes_do() {
        let schemas = [("a", json!({ "type": "string" }))];
        let wire = [entry("ipc.frame_empty"), entry("request.malformed")];
        let base = digest_of(&schemas, wire, LIMITS);
        let with_cli = digest_of(
            &schemas,
            [
                entry("ipc.frame_empty"),
                entry("cli.invalid_flag"),
                entry("request.malformed"),
                entry("cli.no_tty"),
            ],
            LIMITS,
        );
        let with_wire = digest_of(
            &schemas,
            [
                entry("ipc.frame_empty"),
                entry("request.malformed"),
                entry("events.gone"),
            ],
            LIMITS,
        );
        assert_eq!(base, with_cli);
        assert_ne!(base, with_wire);
        assert!(!is_wire_code("cli.invalid_flag"));
        assert!(
            is_wire_code("client.x"),
            "only the area named cli is left out"
        );
    }

    #[test]
    fn the_digest_sees_item_kinds_fields_and_limits_in_any_field_order() {
        let schemas = [("a", json!({ "type": "string" }))];
        let with_items = |fields: &'static [&'static str], max| DigestEntry {
            items: Some(("issue", fields, max)),
            ..entry("manifest.invalid")
        };
        let base = digest_of(&schemas, [with_items(&["path", "code"], 50)], LIMITS);
        let reordered = digest_of(&schemas, [with_items(&["code", "path"], 50)], LIMITS);
        let raised = digest_of(&schemas, [with_items(&["path", "code"], 60)], LIMITS);
        let none = digest_of(&schemas, [entry("manifest.invalid")], LIMITS);
        assert_eq!(base, reordered);
        assert_ne!(base, raised);
        assert_ne!(base, none);
    }

    #[test]
    fn writing_adds_a_missing_entry_and_keeps_a_matching_one() {
        let first = update_versions(
            ContractVersions::default(),
            V1_0,
            "d1",
            VersionsMode::Write { amend: false },
        )
        .unwrap();
        assert_eq!(first, record(&[(1, 0, "d1")]));
        let again = update_versions(
            first.clone(),
            V1_0,
            "d1",
            VersionsMode::Write { amend: false },
        )
        .unwrap();
        assert_eq!(again, first);
        let next =
            update_versions(first, V1_1, "d2", VersionsMode::Write { amend: false }).unwrap();
        assert_eq!(next, record(&[(1, 0, "d1"), (1, 1, "d2")]));
    }

    #[test]
    fn a_changed_digest_without_a_new_version_is_refused_unless_amended() {
        let recorded = record(&[(1, 0, "d1")]);
        let err = update_versions(
            recorded.clone(),
            V1_0,
            "d2",
            VersionsMode::Write { amend: false },
        )
        .unwrap_err();
        assert!(err.to_string().contains("bump CURRENT"), "{err}");
        let amended =
            update_versions(recorded, V1_0, "d2", VersionsMode::Write { amend: true }).unwrap();
        assert_eq!(amended, record(&[(1, 0, "d2")]));
    }

    #[test]
    fn check_fails_on_a_changed_digest_a_missing_entry_or_a_higher_version() {
        let recorded = record(&[(1, 0, "d1")]);
        assert!(update_versions(recorded.clone(), V1_0, "d1", VersionsMode::Check).is_ok());
        assert!(update_versions(recorded.clone(), V1_0, "d2", VersionsMode::Check).is_err());
        assert!(update_versions(recorded, V1_1, "d2", VersionsMode::Check).is_err());
        let ahead = record(&[(1, 0, "d1"), (1, 1, "d2")]);
        let err =
            update_versions(ahead, V1_0, "d1", VersionsMode::Write { amend: true }).unwrap_err();
        assert!(
            err.to_string().contains("above ProtocolVersion::CURRENT"),
            "{err}"
        );
    }

    #[test]
    fn the_error_codes_file_is_sorted_by_code() {
        let file = error_codes_file();
        let codes: Vec<&str> = file.codes.iter().map(|entry| entry.code).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted);
        assert_eq!(codes.len(), codes::catalog().count());
        assert_eq!(file.areas.len(), codes::AREAS.len());
    }
}
