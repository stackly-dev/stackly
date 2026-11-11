//! Check (d) of `cargo xtask docs check` (spec 0038, AC-10): each reference
//! page names, as a whole code span, every property name and every string
//! enum value of its JSON Schema. The check runs, and the three pages must
//! exist, once `docs/providers/README.md` exists; before that it is skipped.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use super::Problem;
use super::markdown::{Markdown, code_spans};

/// The page whose presence turns the check on.
const PROVIDER_GUIDE: &str = "docs/providers/README.md";

/// Each schema and the reference page that covers it.
const PAIRS: [(&str, &str); 3] = [
    (
        "schemas/provider/provider.v1alpha1.schema.json",
        "docs/providers/manifest-reference.md",
    ),
    (
        "schemas/provider/settings.v1alpha1.schema.json",
        "docs/providers/settings-reference.md",
    ),
    (
        "schemas/provider/versions.v1.schema.json",
        "docs/providers/versions-reference.md",
    ),
];

/// The coverage problems of the repo at `root`.
///
/// # Errors
///
/// Returns an error when a schema the check needs cannot be read or is not
/// JSON.
pub fn check(root: &Path) -> Result<Vec<Problem>> {
    if !root.join(PROVIDER_GUIDE).is_file() {
        return Ok(Vec::new());
    }
    let mut problems = Vec::new();
    for (schema_rel, page_rel) in PAIRS {
        let schema_path = root.join(schema_rel);
        let schema: Value = serde_json::from_str(
            &fs::read_to_string(&schema_path)
                .with_context(|| format!("cannot read {}", schema_path.display()))?,
        )
        .with_context(|| format!("{} is not JSON", schema_path.display()))?;
        let Ok(page) = fs::read_to_string(root.join(page_rel)) else {
            problems.push(Problem::new(
                page_rel,
                1,
                format!("reference page is missing; it must cover `{schema_rel}`"),
            ));
            continue;
        };
        let mentioned = mentioned(&page);
        let names = names(&schema);
        for name in names.properties.difference(&mentioned) {
            problems.push(Problem::new(
                page_rel,
                1,
                format!("property `{name}` of `{schema_rel}` is not mentioned"),
            ));
        }
        for value in names.values.difference(&mentioned) {
            problems.push(Problem::new(
                page_rel,
                1,
                format!("enum value `{value}` of `{schema_rel}` is not mentioned"),
            ));
        }
    }
    Ok(problems)
}

/// Every code span of `page`, in prose and outside fenced blocks.
fn mentioned(page: &str) -> BTreeSet<String> {
    let doc = Markdown::parse(page);
    doc.lines
        .iter()
        .filter(|line| line.prose.is_some())
        .flat_map(|line| code_spans(&line.raw))
        .collect()
}

/// The names a schema defines.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Names {
    /// Every key of every `properties` object.
    pub properties: BTreeSet<String>,
    /// Every string in an `enum` or a `const`.
    pub values: BTreeSet<String>,
}

/// The names of `schema`, walking only schema positions: the root, every
/// value of `properties` and `$defs`, `items`, `additionalProperties` when
/// it is an object, and every element of `anyOf`, `oneOf`, and `allOf`.
#[must_use]
pub fn names(schema: &Value) -> Names {
    let mut names = Names::default();
    walk(schema, &mut names);
    names
}

/// Collects the names at one schema position and below.
fn walk(position: &Value, names: &mut Names) {
    let Some(object) = position.as_object() else {
        return;
    };
    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        for (name, child) in properties {
            names.properties.insert(name.clone());
            walk(child, names);
        }
    }
    if let Some(defs) = object.get("$defs").and_then(Value::as_object) {
        for child in defs.values() {
            walk(child, names);
        }
    }
    match object.get("items") {
        Some(Value::Array(items)) => items.iter().for_each(|child| walk(child, names)),
        Some(child) => walk(child, names),
        None => {}
    }
    if let Some(child) = object.get("additionalProperties")
        && child.is_object()
    {
        walk(child, names);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(children) = object.get(key).and_then(Value::as_array) {
            children.iter().for_each(|child| walk(child, names));
        }
    }
    if let Some(values) = object.get("enum").and_then(Value::as_array) {
        names
            .values
            .extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    if let Some(value) = object.get("const").and_then(Value::as_str) {
        names.values.insert(value.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::fixture;
    use super::names;

    #[test]
    fn names_walk_only_schema_positions() {
        let schema = json!({
            "properties": {
                "kind": {"enum": ["a", "b", 3]},
                "enum": {"const": "c"},
                "list": {"items": {"$ref": "#/$defs/item"}},
                "map": {"additionalProperties": {"properties": {"inner": {}}}},
                "example": {"default": {"properties": {"ignored": {}}}}
            },
            "$defs": {"item": {"oneOf": [{"properties": {"name": {}}}, {"const": "d"}]}}
        });
        let found = names(&schema);
        let properties: Vec<_> = found.properties.iter().map(String::as_str).collect();
        assert_eq!(
            properties,
            vec!["enum", "example", "inner", "kind", "list", "map", "name"]
        );
        let values: Vec<_> = found.values.iter().map(String::as_str).collect();
        assert_eq!(values, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn coverage_is_skipped_until_the_provider_guide_exists() {
        let repo = fixture::repo();
        assert!(fixture::lines(repo.path()).is_empty());
    }

    #[test]
    fn a_property_the_page_does_not_name_is_one_problem() {
        let repo = fixture::repo();
        let root = repo.path();
        fixture::write(root, "docs/providers/README.md", "# Providers\n");
        let schema = r#"{"properties":{"apiVersion":{"const":"stackly/v1alpha1"},"kind":{}}}"#;
        for name in ["provider.v1alpha1", "settings.v1alpha1", "versions.v1"] {
            fixture::write(
                root,
                &format!("schemas/provider/{name}.schema.json"),
                schema,
            );
        }
        let page = "# Reference\n\n`apiVersion` is `stackly/v1alpha1`. `kind` names it.\n";
        fixture::write(root, "docs/providers/manifest-reference.md", page);
        fixture::write(root, "docs/providers/settings-reference.md", page);
        fixture::write(
            root,
            "docs/providers/versions-reference.md",
            "# Versions\n\n`apiVersion` is `stackly/v1alpha1`.\n\n```text\n`kind`\n```\n",
        );
        assert_eq!(
            fixture::lines(root),
            vec![
                "docs/providers/versions-reference.md:1: property `kind` of `schemas/provider/versions.v1.schema.json` is not mentioned"
            ]
        );
    }
}
