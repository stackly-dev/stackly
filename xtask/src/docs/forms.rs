//! The YAML part of `cargo xtask docs check` (spec 0038): checks (a) and (b)
//! on every string value of a `.github/` YAML file, and check (e), the
//! structure of the issue forms and of the issue chooser `config.yml`.

use std::collections::BTreeSet;
use std::path::Path;

use serde_yaml_ng::Value;

use super::links::{self, Checker};
use super::markdown::Markdown;
use super::text;
use super::{ADVISORY_URL, Problem, TRIAGE_LABELS};

/// The element types an issue form body may hold.
const ELEMENT_TYPES: [&str; 5] = ["markdown", "input", "textarea", "dropdown", "checkboxes"];

/// Links and placeholders on every string value of the YAML file `rel`.
/// A file that is not YAML gets no problem here; check (e) reports it for
/// an issue form.
pub fn check_strings(rel: &str, abs: &Path, raw: &str, links: &mut Checker) -> Vec<Problem> {
    let Ok(value) = serde_yaml_ng::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let mut strings = Vec::new();
    collect_strings(&value, &mut strings);
    let mut problems = Vec::new();
    for string in strings {
        let doc = Markdown::parse(&string);
        for (index, prose) in doc.prose() {
            let line = line_in(raw, &doc.lines[index - 1].raw);
            for dest in links::destinations(prose) {
                if let Some(message) = links.problem(abs, &[], &dest) {
                    problems.push(Problem::new(rel, line, message));
                }
            }
            for found in text::placeholders_in(prose) {
                problems.push(Problem::new(rel, line, format!("placeholder `{found}`")));
            }
        }
    }
    problems
}

/// Every string scalar under `value`.
fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Sequence(items) => items.iter().for_each(|item| collect_strings(item, out)),
        Value::Mapping(map) => map.values().for_each(|item| collect_strings(item, out)),
        Value::Tagged(tagged) => collect_strings(&tagged.value, out),
        _ => {}
    }
}

/// The 1 based line of `raw` that holds `needle`, or 1.
fn line_in(raw: &str, needle: &str) -> usize {
    let needle = needle.trim();
    if needle.is_empty() {
        return 1;
    }
    raw.lines()
        .position(|line| line.contains(needle))
        .map_or(1, |index| index + 1)
}

/// Check (e) for one file in `.github/ISSUE_TEMPLATE/`.
#[must_use]
pub fn check_form(rel: &str, raw: &str) -> Vec<Problem> {
    let value = match serde_yaml_ng::from_str::<Value>(raw) {
        Ok(value) => value,
        Err(err) => {
            let line = err.location().map_or(1, |location| location.line());
            return vec![Problem::new(rel, line, format!("not valid YAML: {err}"))];
        }
    };
    if rel.ends_with("/config.yml") {
        return check_config(rel, raw, &value);
    }
    let mut problems = Vec::new();
    let mut problem = |needle: &str, message: String| {
        problems.push(Problem::new(rel, line_in(raw, needle), message));
    };
    for key in ["name", "description"] {
        if value
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            problem("", format!("issue form has no `{key}`"));
        }
    }
    for label in labels(&value) {
        if !TRIAGE_LABELS.contains(&label.as_str()) {
            problem(&label, format!("label `{label}` is not a triage label"));
        }
    }
    let Some(body) = value
        .get("body")
        .and_then(Value::as_sequence)
        .filter(|body| !body.is_empty())
    else {
        problem("", "issue form has no `body` elements".to_owned());
        return problems;
    };
    let mut ids = BTreeSet::new();
    for (index, element) in body.iter().enumerate() {
        let at = format!("body element {}", index + 1);
        let kind = element
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = element.get("id").and_then(Value::as_str);
        let needle = id.map_or_else(|| format!("type: {kind}"), |id| format!("id: {id}"));
        if !ELEMENT_TYPES.contains(&kind) {
            problem(
                &needle,
                format!(
                    "{at} has type `{kind}`, not one of {}",
                    ELEMENT_TYPES.join(", ")
                ),
            );
            continue;
        }
        let attributes = element.get("attributes");
        let attribute = |name: &str| attributes.and_then(|attributes| attributes.get(name));
        if kind == "markdown" {
            if attribute("value")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                problem(
                    &needle,
                    format!("{at} (markdown) has no `attributes.value`"),
                );
            }
            continue;
        }
        match id {
            Some(id) if !id.is_empty() => {
                if !ids.insert(id.to_owned()) {
                    problem(&needle, format!("{at} repeats the id `{id}`"));
                }
            }
            _ => problem(&needle, format!("{at} ({kind}) has no `id`")),
        }
        if attribute("label")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            problem(&needle, format!("{at} ({kind}) has no `attributes.label`"));
        }
        if matches!(kind, "dropdown" | "checkboxes")
            && attribute("options")
                .and_then(Value::as_sequence)
                .is_none_or(Vec::is_empty)
        {
            problem(
                &needle,
                format!("{at} ({kind}) has no `attributes.options`"),
            );
        }
    }
    problems
}

/// The labels of a form, from a list or a comma separated string.
fn labels(form: &Value) -> Vec<String> {
    match form.get("labels") {
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(|label| label.trim().to_owned())
            .filter(|label| !label.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Check (e) for the issue chooser `config.yml`.
fn check_config(rel: &str, raw: &str, value: &Value) -> Vec<Problem> {
    let mut problems = Vec::new();
    if value.get("blank_issues_enabled").and_then(Value::as_bool) != Some(false) {
        problems.push(Problem::new(
            rel,
            line_in(raw, "blank_issues_enabled"),
            "`blank_issues_enabled` must be false",
        ));
    }
    let links_to_advisory = value
        .get("contact_links")
        .and_then(Value::as_sequence)
        .is_some_and(|links| {
            links
                .iter()
                .any(|link| link.get("url").and_then(Value::as_str) == Some(ADVISORY_URL))
        });
    if !links_to_advisory {
        problems.push(Problem::new(
            rel,
            line_in(raw, "contact_links"),
            format!("`contact_links` has no link to `{ADVISORY_URL}`"),
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::super::fixture;

    const FORM: &str = "name: Bug\ndescription: Something does not work\nlabels: [needs-triage]\nbody:\n  - type: markdown\n    attributes:\n      value: Read [the guide](../../CONTRIBUTING.md) first.\n  - type: dropdown\n    id: area\n    attributes:\n      label: Area\n      options:\n        - Core\n  - type: textarea\n    id: steps\n    attributes:\n      label: Steps\n";

    const CONFIG: &str = "blank_issues_enabled: false\ncontact_links:\n  - name: Report a security vulnerability\n    url: https://github.com/stackly-dev/stackly/security/advisories/new\n    about: Private report\n";

    fn with(form: &str) -> Vec<String> {
        let repo = fixture::repo();
        fixture::write(repo.path(), ".github/ISSUE_TEMPLATE/config.yml", CONFIG);
        fixture::write(repo.path(), ".github/ISSUE_TEMPLATE/bug.yml", form);
        fixture::lines(repo.path())
    }

    #[test]
    fn a_valid_form_and_config_have_no_problems() {
        assert_eq!(with(FORM), Vec::<String>::new());
    }

    #[test]
    fn a_form_without_body_is_one_problem() {
        let form = "name: Bug\ndescription: Something\nlabels: [needs-triage]\n";
        assert_eq!(
            with(form),
            vec![".github/ISSUE_TEMPLATE/bug.yml:1: issue form has no `body` elements"]
        );
    }

    #[test]
    fn a_label_outside_the_triage_table_is_one_problem() {
        let form = FORM.replace("[needs-triage]", "[needs-triage, bug]");
        assert_eq!(
            with(&form),
            vec![".github/ISSUE_TEMPLATE/bug.yml:3: label `bug` is not a triage label"]
        );
    }

    #[test]
    fn a_repeated_id_a_missing_label_and_empty_options_are_problems() {
        let form = FORM
            .replace("id: steps", "id: area")
            .replace("      label: Steps\n", "      placeholder: x\n")
            .replace("      options:\n        - Core\n", "      options: []\n");
        let lines = with(&form);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(
            lines[0].contains("has no `attributes.options`"),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("repeats the id `area`"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("(textarea) has no `attributes.label`"))
        );
    }

    #[test]
    fn a_broken_link_or_placeholder_in_a_string_value_is_a_problem() {
        let form = FORM.replace("../../CONTRIBUTING.md) first.", "../../NOPE.md) first. TBD");
        assert_eq!(
            with(&form),
            vec![
                ".github/ISSUE_TEMPLATE/bug.yml:7: link target `../../NOPE.md` does not exist",
                ".github/ISSUE_TEMPLATE/bug.yml:7: placeholder `TBD`",
            ]
        );
    }

    #[test]
    fn a_config_with_blank_issues_and_no_advisory_link_has_two_problems() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            ".github/ISSUE_TEMPLATE/config.yml",
            "blank_issues_enabled: true\ncontact_links: []\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec![
                ".github/ISSUE_TEMPLATE/config.yml:1: `blank_issues_enabled` must be false",
                ".github/ISSUE_TEMPLATE/config.yml:2: `contact_links` has no link to `https://github.com/stackly-dev/stackly/security/advisories/new`",
            ]
        );
    }

    #[test]
    fn a_form_that_is_not_yaml_is_one_problem() {
        let lines = with("name: [unclosed\n");
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with(".github/ISSUE_TEMPLATE/bug.yml:"));
        assert!(lines[0].contains("not valid YAML"));
    }
}
