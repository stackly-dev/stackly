//! The `stackly.yaml` document (spec 0044, *Schema `stackly.yaml`*): the
//! raw serde types, which also generate the editor JSON Schema, and the
//! strict loader that turns bytes into them.
//!
//! The loader refuses what YAML allows but the manifest does not: unknown
//! fields, repeated keys, anchors, aliases, tags, merge keys, and another
//! `apiVersion` or `kind`. Every finding it makes carries a fixed message
//! (AC-3): the text of a `serde_yaml_ng` error only chooses the code and
//! the location, and a key from the file reaches a location only when it
//! looks like a name.

use serde::Deserialize;

use super::finding::{Finding, FindingCode, FindingLocation, Findings};
use crate::provider::raw::{Ordered, YamlError, yaml_value};

/// The one `apiVersion` this build reads.
pub const API_VERSION: &str = "stackly/v1alpha1";
/// The `kind` of an environment manifest.
pub const KIND: &str = "Environment";
/// Every `apiVersion` this build reads, newest first.
pub const SUPPORTED_API_VERSIONS: &[&str] = &[API_VERSION];
/// Largest manifest file, in bytes (AC-1).
pub const MANIFEST_MAX_LEN: u64 = 256 * 1024;

/// `stackly.yaml`, as it decodes.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Environment"))]
pub(crate) struct RawEnvironment {
    /// Always `stackly/v1alpha1`.
    #[expect(
        dead_code,
        reason = "the loader checks it before decoding; the schema shows it"
    )]
    #[cfg_attr(
        feature = "json-schema",
        schemars(regex(pattern = r"^stackly/v1alpha1$"))
    )]
    pub(crate) api_version: String,
    /// Always `Environment`.
    #[expect(
        dead_code,
        reason = "the loader checks it before decoding; the schema shows it"
    )]
    #[cfg_attr(feature = "json-schema", schemars(regex(pattern = r"^Environment$")))]
    pub(crate) kind: String,
    /// The environment's identity.
    pub(crate) metadata: RawMetadata,
    /// The services, by alias, in document order: 1 to 32.
    pub(crate) services: Ordered<RawService>,
}

/// `metadata`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Metadata"))]
pub(crate) struct RawMetadata {
    /// The environment name, unique on this Mac.
    #[cfg_attr(
        feature = "json-schema",
        schemars(length(min = 1, max = 63), regex(pattern = crate::provider::json_schema::NAME))
    )]
    pub(crate) name: String,
}

/// One service.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Service"))]
pub(crate) struct RawService {
    /// `<namespace>/<name>` of a catalog provider.
    #[cfg_attr(
        feature = "json-schema",
        schemars(regex(pattern = crate::provider::json_schema::PROVIDER_NAME))
    )]
    pub(crate) provider: String,
    /// A runtime selector, always a string: `stable`, a line, an exact
    /// version, or `sha256:<hex>`. Dedicated services only.
    #[serde(default)]
    pub(crate) version: Option<String>,
    /// `dedicated` (the default) or `shared` (only with `instanceRef`).
    #[serde(default)]
    #[cfg_attr(
        feature = "json-schema",
        schemars(regex(pattern = r"^(dedicated|shared)$"))
    )]
    pub(crate) ownership: Option<String>,
    /// The name of an existing instance this service borrows.
    #[serde(default)]
    pub(crate) instance_ref: Option<String>,
    /// The name of the instance to create; `<metadata.name>-<alias>` by
    /// default.
    #[serde(default)]
    pub(crate) instance_name: Option<String>,
    /// Endpoint ports by endpoint name; an endpoint not written is `auto`.
    #[serde(default)]
    pub(crate) endpoints: Option<Ordered<RawEndpoint>>,
    /// Settings, checked against the provider's settings schema.
    #[serde(default)]
    pub(crate) settings: Option<serde_json::Map<String, serde_json::Value>>,
    /// `local:` secret references by secret field.
    #[serde(default)]
    pub(crate) secret_refs: Option<Ordered<String>>,
    /// Only `{mode: managed, persistent: true}` in v1alpha1.
    #[serde(default)]
    pub(crate) storage: Option<RawStorage>,
    /// Services that must be ready first: at most 31.
    #[serde(default)]
    pub(crate) depends_on: Option<Vec<RawDependency>>,
}

/// One endpoint of a service.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Endpoint"))]
pub(crate) struct RawEndpoint {
    /// A port from 1 to 65535, or `auto`.
    #[serde(default)]
    pub(crate) port: Option<RawPort>,
    /// When written, the endpoint's default host.
    #[serde(default)]
    pub(crate) host: Option<String>,
    /// When written, the endpoint's transport.
    #[serde(default)]
    pub(crate) transport: Option<String>,
}

/// A port: a number, or the text `auto`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
#[cfg_attr(feature = "json-schema", schemars(rename = "Port"))]
pub(crate) enum RawPort {
    /// A fixed port.
    Fixed(#[cfg_attr(feature = "json-schema", schemars(range(min = 1, max = 65535)))] i64),
    /// `auto`.
    Text(#[cfg_attr(feature = "json-schema", schemars(regex(pattern = r"^auto$")))] String),
}

/// `storage`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Storage"))]
pub(crate) struct RawStorage {
    /// Only `managed`.
    #[serde(default)]
    #[cfg_attr(feature = "json-schema", schemars(regex(pattern = r"^managed$")))]
    pub(crate) mode: Option<String>,
    /// Only `true`.
    #[serde(default)]
    pub(crate) persistent: Option<bool>,
}

/// One `dependsOn` entry.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[cfg_attr(feature = "json-schema", schemars(rename = "Dependency"))]
pub(crate) struct RawDependency {
    /// The alias of another service in this manifest.
    pub(crate) service: String,
    /// Only `ready`, the default.
    #[serde(default)]
    #[cfg_attr(feature = "json-schema", schemars(regex(pattern = r"^ready$")))]
    pub(crate) condition: Option<String>,
    /// How long to wait, 1 to 3600 seconds; the default of Up otherwise.
    #[serde(default)]
    #[cfg_attr(feature = "json-schema", schemars(range(min = 1, max = 3600)))]
    pub(crate) timeout_seconds: Option<i64>,
}

/// The JSON Schema of `stackly.yaml` for editors, with the file stem
/// `environment.v1alpha1` that `cargo xtask codegen` writes it under in
/// `schemas/environment/` (spec 0044 AC-24). It describes structure and
/// names; the validator stays the authority.
#[cfg(feature = "json-schema")]
#[must_use]
pub fn json_schemas() -> Vec<(&'static str, schemars::Schema)> {
    let generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    vec![(
        "environment.v1alpha1",
        generator.into_root_schema_for::<RawEnvironment>(),
    )]
}

/// Decodes manifest bytes, or returns the findings that stopped it. The
/// shape of the document is checked first and the first problem stops it
/// (spec 0044, *Aturan validasi*), except anchors and aliases, which are
/// all reported.
pub(crate) fn load(bytes: &[u8]) -> Result<RawEnvironment, Findings> {
    load_document(bytes, KIND, FindingCode::UnsupportedKind.template())
}

/// The strict loader of every environment file (spec 0044, and the lock
/// and the local overrides of spec 0045): bytes into `T`, or the findings
/// that stopped it. `kind` is the one `kind` the document may have, and
/// `kind_message` the message of `unsupported_kind`.
pub(crate) fn load_document<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    kind: &str,
    kind_message: &'static str,
) -> Result<T, Findings> {
    let mut findings = Findings::default();
    let stop = |findings: &mut Findings, finding: Finding| {
        findings.push(finding);
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        stop(
            &mut findings,
            Finding::error(FindingCode::YamlSyntax, FindingLocation::File),
        );
        return Err(findings);
    };
    let value = match yaml_value(text) {
        Ok(value) => value,
        Err(err) => {
            let (code, location) = match err {
                YamlError::TooLarge { .. } => (FindingCode::YamlAlias, FindingLocation::File),
                YamlError::Parse(err) => {
                    let location = err.location().map_or(FindingLocation::File, |at| {
                        FindingLocation::Position {
                            line: at.line(),
                            column: at.column(),
                        }
                    });
                    let message = err.to_string();
                    let code = if message.contains("duplicate entry") {
                        FindingCode::DuplicateKey
                    } else if message.contains("unknown anchor") {
                        FindingCode::YamlAlias
                    } else {
                        FindingCode::YamlSyntax
                    };
                    (code, location)
                }
            };
            stop(&mut findings, Finding::error(code, location));
            return Err(findings);
        }
    };
    for (line, column) in anchors_and_aliases(text) {
        findings.push(Finding::error(
            FindingCode::YamlAlias,
            FindingLocation::Position { line, column },
        ));
    }
    if let Some(path) = tag_or_merge_key(&value, "") {
        findings.push(Finding::error(
            FindingCode::YamlAlias,
            FindingLocation::Field(path),
        ));
    }
    if findings.count() > 0 {
        return Err(findings);
    }
    let Some(mapping) = value.as_mapping() else {
        stop(
            &mut findings,
            Finding::error(
                FindingCode::InvalidValue,
                FindingLocation::Field(".".into()),
            ),
        );
        return Err(findings);
    };
    match mapping.get("apiVersion") {
        None => {
            stop(
                &mut findings,
                Finding::error(
                    FindingCode::MissingField,
                    FindingLocation::Field("apiVersion".into()),
                ),
            );
            return Err(findings);
        }
        Some(found) if found.as_str() == Some(API_VERSION) => {}
        Some(_) => {
            stop(
                &mut findings,
                Finding::error(
                    FindingCode::UnsupportedApiVersion,
                    FindingLocation::Field("apiVersion".into()),
                ),
            );
            return Err(findings);
        }
    }
    match mapping.get("kind") {
        None => {
            stop(
                &mut findings,
                Finding::error(
                    FindingCode::MissingField,
                    FindingLocation::Field("kind".into()),
                ),
            );
            return Err(findings);
        }
        Some(found) if found.as_str() == Some(kind) => {}
        Some(_) => {
            stop(
                &mut findings,
                Finding::error(
                    FindingCode::UnsupportedKind,
                    FindingLocation::Field("kind".into()),
                )
                .with_message(kind_message),
            );
            return Err(findings);
        }
    }
    serde_path_to_error::deserialize::<_, T>(value).map_err(|err| {
        let path = err.path().to_string();
        let inner = err.inner().to_string();
        findings.push(decode_finding(&path, &inner));
        findings
    })
}

/// Turns a decode error at `path` into a finding with a fixed message.
fn decode_finding(path: &str, message: &str) -> Finding {
    let path = if path == "." { "" } else { path };
    let named = |prefix: &str| {
        message
            .strip_prefix(prefix)
            .and_then(|rest| rest.split('`').next())
            .map(|name| join(&safe_path(path), &safe_segment(name)))
    };
    let (code, location) = if message.starts_with("unknown field `") {
        (FindingCode::UnknownField, safe_path(path))
    } else if let Some(at) = named("missing field `") {
        (FindingCode::MissingField, at)
    } else if let Some(at) = named("duplicate field `") {
        (FindingCode::DuplicateKey, at)
    } else {
        (FindingCode::InvalidValue, safe_path(path))
    };
    let location = if location.is_empty() {
        ".".to_owned()
    } else {
        location
    };
    let finding = Finding::error(code, FindingLocation::Field(location.clone()));
    if code == FindingCode::InvalidValue && location.ends_with(".version") {
        finding.with_detail("write the version in quotes")
    } else {
        finding
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

/// A path of names and indexes, with every segment that does not look
/// like a name replaced by `?`.
pub(crate) fn safe_path(path: &str) -> String {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .map(|segment| match segment.split_once('[') {
            Some((name, index)) if index.ends_with(']') => {
                let digits = &index[..index.len() - 1];
                if digits.bytes().all(|b| b.is_ascii_digit()) {
                    format!("{}[{digits}]", safe_segment(name))
                } else {
                    "?".to_owned()
                }
            }
            _ => safe_segment(segment),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// A key from the file, kept only when it looks like a name.
pub(crate) fn safe_segment(segment: &str) -> String {
    let looks_like_a_name = !segment.is_empty()
        && segment.len() <= 64
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if looks_like_a_name {
        segment.to_owned()
    } else {
        "?".to_owned()
    }
}

/// The path of the first tag or merge key in `value`, if any.
fn tag_or_merge_key(value: &serde_yaml_ng::Value, path: &str) -> Option<String> {
    use serde_yaml_ng::Value;
    let here = || {
        if path.is_empty() {
            ".".to_owned()
        } else {
            path.to_owned()
        }
    };
    match value {
        Value::Tagged(_) => Some(here()),
        Value::Sequence(items) => items
            .iter()
            .enumerate()
            .find_map(|(index, item)| tag_or_merge_key(item, &format!("{path}[{index}]"))),
        Value::Mapping(map) => map.iter().find_map(|(key, item)| {
            let name = match key {
                Value::Tagged(_) => return Some(here()),
                Value::String(name) => safe_segment(name),
                _ => "?".to_owned(),
            };
            let child = join(path, &name);
            if key.as_str() == Some("<<") {
                return Some(child);
            }
            tag_or_merge_key(item, &child)
        }),
        _ => None,
    }
}

/// The line and column (from 1) of every anchor (`&name`) and alias
/// (`*name`) in YAML `text`, which already parsed, and of every quote the
/// scan can't follow.
///
/// `serde_yaml_ng` expands aliases without saying so, so this scans the
/// text: outside quoted scalars, block scalars, and comments, an `&` or
/// `*` that starts a node is an anchor or an alias, because a plain scalar
/// can't start with either. The scan is conservative (K2): a quote may
/// span lines only when it clearly opens a value (`key: "` or `- "` on a
/// line that can't continue a plain scalar). Any other quote that does
/// not close on its own line is reported too, so a file the scan might
/// misread is refused rather than letting an anchor or alias through.
pub(crate) fn anchors_and_aliases(text: &str) -> Vec<(usize, usize)> {
    /// Where a block scalar is.
    #[derive(Clone, Copy)]
    enum Block {
        /// Opened under a node at this column; no content line yet.
        Opened { node: usize },
        /// Its content lines are indented at least this far.
        Content { indent: usize },
    }
    /// An open quoted scalar.
    #[derive(Clone, Copy)]
    struct Quote {
        /// `"` or `'`.
        mark: char,
        /// Where it opened, from 1.
        at: (usize, usize),
        /// Whether it may span lines.
        multi_line: bool,
    }
    let mut found = Vec::new();
    let mut quote: Option<Quote> = None;
    let mut block: Option<Block> = None;
    // When the last content line ended in a scalar, the column a line
    // must be indented past to continue it.
    let mut plain_tail: Option<usize> = None;
    for (line_index, line) in text.lines().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let indent = chars.iter().take_while(|c| **c == ' ').count();
        let blank = chars.iter().all(|c| c.is_whitespace());
        match block {
            _ if blank && block.is_some() => continue,
            // The first line that is not blank sets the content indent; a
            // line no deeper than the node leaves the scalar empty.
            Some(Block::Opened { node }) if indent > node => {
                block = Some(Block::Content { indent });
                continue;
            }
            Some(Block::Content { indent: content }) if indent >= content => continue,
            _ => block = None,
        }
        let continues = quote.is_none() && !blank && plain_tail.is_some_and(|tail| indent > tail);
        // The column of the node this line starts: past the indent and any
        // `- ` sequence markers.
        let mut node = indent;
        while chars.get(node) == Some(&'-') && chars.get(node + 1).is_none_or(|c| *c == ' ') {
            node += 1;
            while chars.get(node) == Some(&' ') {
                node += 1;
            }
        }
        let mut has_key = false;
        // Whether the last significant character ended a scalar rather
        // than being an indicator, and that character.
        let mut last_significant: Option<char> = None;
        let mut structural = true;
        let mut significant = false;
        let mut index = 0;
        while index < chars.len() {
            let c = chars[index];
            if let Some(open) = quote {
                if open.mark == '"' && c == '\\' {
                    index += 2;
                    continue;
                }
                if c == open.mark {
                    if open.mark == '\'' && chars.get(index + 1) == Some(&'\'') {
                        index += 2;
                        continue;
                    }
                    quote = None;
                    last_significant = Some(c);
                    structural = false;
                    significant = true;
                }
                index += 1;
                continue;
            }
            let previous = index.checked_sub(1).map(|at| chars[at]);
            if c == '#' && previous.is_none_or(char::is_whitespace) {
                break;
            }
            if c.is_whitespace() {
                index += 1;
                continue;
            }
            significant = true;
            let followed_by_space = chars.get(index + 1).is_none_or(|next| next.is_whitespace());
            let starts_node = match previous {
                None => true,
                Some('[' | '{' | ',') => true,
                Some(p) if p.is_whitespace() => {
                    matches!(
                        last_significant,
                        None | Some('-' | '?' | ':' | ',' | '[' | '{')
                    )
                }
                Some(_) => false,
            };
            structural = match c {
                ':' => followed_by_space,
                '-' | '?' => starts_node && followed_by_space,
                '[' | '{' | ',' => true,
                _ => false,
            };
            if c == ':' && followed_by_space {
                has_key = true;
            }
            if starts_node {
                match c {
                    '&' | '*' => {
                        let named = chars.get(index + 1).is_some_and(|next| {
                            !next.is_whitespace() && !matches!(next, ',' | '[' | ']' | '{' | '}')
                        });
                        if named {
                            found.push((line_index + 1, index + 1));
                        }
                    }
                    '"' | '\'' => {
                        let clear = previous.is_some_and(char::is_whitespace)
                            && matches!(last_significant, Some(':' | '-'))
                            && !continues;
                        quote = Some(Quote {
                            mark: c,
                            at: (line_index + 1, index + 1),
                            multi_line: clear,
                        });
                    }
                    '|' | '>' => {
                        let rest: String = chars[index + 1..].iter().collect();
                        let header = rest.split('#').next().unwrap_or_default().trim();
                        if header
                            .chars()
                            .all(|h| h.is_ascii_digit() || h == '+' || h == '-')
                        {
                            // A mapping value's block sits under its key;
                            // a sequence entry's under the line.
                            let parent = if has_key { node } else { indent };
                            let explicit = header
                                .chars()
                                .find(char::is_ascii_digit)
                                .and_then(|digit| digit.to_digit(10));
                            block = Some(match explicit {
                                Some(step) => Block::Content {
                                    indent: parent + step as usize,
                                },
                                None => Block::Opened { node: parent },
                            });
                            break;
                        }
                    }
                    _ => {}
                }
            }
            last_significant = Some(c);
            index += 1;
        }
        if let Some(open) = quote
            && !open.multi_line
        {
            // A quote the scan can't follow across lines.
            found.push(open.at);
            quote = None;
        }
        if block.is_some() {
            plain_tail = None;
        } else if significant && quote.is_none() {
            // A mapping value continues only past its key's column; a
            // sequence entry past the line's indent.
            plain_tail = (!structural).then_some(if has_key { node } else { indent });
        }
    }
    if let Some(open) = quote {
        found.push(open.at);
    }
    found.sort_unstable();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_and_aliases_are_found_outside_quotes_comments_and_block_scalars() {
        let text = "\
a: &base {x: 1}
b: *base
c: \"&not *an alias\"
d: 'it''s &fine'
# &comment *too
e: |
  &inside *block
f: [*base, &other 2]
g: a&b*c
h:
  - *base
";
        assert_eq!(
            anchors_and_aliases(text),
            [(1, 4), (2, 4), (8, 5), (8, 12), (11, 5)]
        );
    }

    #[test]
    fn a_key_after_a_block_scalar_in_a_sequence_item_is_scanned_again() {
        let text = "\
services:
  api:
    dependsOn:
    - service: |-
        db
      condition: &c ready
  web:
    dependsOn:
    - service: api
      condition: *c
";
        assert_eq!(anchors_and_aliases(text), [(6, 18), (10, 18)]);
    }

    #[test]
    fn a_block_scalar_ends_at_the_first_line_less_indented_than_its_content() {
        let text = "\
a: >
    &deep
    *deep

    still *inside
  b: *out
c: |
d: *empty
";
        assert_eq!(anchors_and_aliases(text), [(6, 6), (8, 4)]);
    }

    #[test]
    fn a_quote_the_scan_cant_follow_across_lines_is_reported() {
        // Line start, a plain scalar continuation, and an unclosed quote at
        // the end of the text.
        let text = "\
a: one
  \"two
b: &x 1
c: x
  'y
d: \"open
  still open
";
        assert_eq!(anchors_and_aliases(text), [(2, 3), (3, 4), (5, 3), (6, 4)]);
    }

    #[test]
    fn a_value_quote_may_span_lines_and_a_quote_inside_plain_text_is_text() {
        let text = "\
a: \"one &x
  two *y\"
b: 'it''s
  &fine'
c: don't \"stop
d:
  - \"three
    four\"
";
        assert!(anchors_and_aliases(text).is_empty());
    }

    #[test]
    fn an_indentation_indicator_sets_the_block_content_under_its_key() {
        let text = "\
a:
  - b: |2-
        deep
      \"shallow but inside
    c: &x 1
";
        assert_eq!(anchors_and_aliases(text), [(5, 8)]);
    }

    #[test]
    fn a_path_keeps_only_segments_that_look_like_names() {
        assert_eq!(
            safe_path("services.postgres.endpoints[0].port"),
            "services.postgres.endpoints[0].port"
        );
        assert_eq!(safe_path("services.has space.x"), "services.?.x");
        assert_eq!(safe_path("a.b[x]"), "a.?");
    }
}
