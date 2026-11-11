//! Check (f) of `cargo xtask docs check` (spec 0038, AC-13): every package in
//! `providers/` outside the `testing` namespace has a `LICENSE-NOTICES.md`
//! with the three required headings, the runtime licenses of
//! `runtimes/inventory.toml` as code spans, and the origin sentence that
//! matches the inventory.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::Problem;
use super::markdown::{Markdown, code_spans};

/// The inventory file, relative to the root.
const INVENTORY: &str = "runtimes/inventory.toml";
/// The required level 2 headings, in order.
const HEADINGS: [&str; 3] = ["Package", "Runtime", "Third party material in this package"];
/// The origin sentence of a binary downloaded from upstream.
const UPSTREAM: &str = "Downloaded from the upstream release";
/// The origin sentence of a binary maintainers build.
const MAINTAINER: &str = "Built by Stackly maintainers from upstream source";

/// The license and origin of each inventory artifact.
#[derive(Debug, Clone, Default)]
pub struct Inventory {
    /// `(provider, license, origin)` of every artifact.
    artifacts: Vec<(String, String, String)>,
}

impl Inventory {
    /// Reads `runtimes/inventory.toml` under `root`.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or is not TOML.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(INVENTORY);
        let text =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        let value: toml::Table =
            toml::from_str(&text).with_context(|| format!("{} is not TOML", path.display()))?;
        let field = |artifact: &toml::Value, key: &str| {
            artifact
                .get(key)
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let artifacts = value
            .get("artifact")
            .and_then(toml::Value::as_array)
            .map(|artifacts| {
                artifacts
                    .iter()
                    .map(|artifact| {
                        (
                            field(artifact, "provider"),
                            field(artifact, "license"),
                            field(artifact, "origin"),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self { artifacts })
    }
}

/// The notice problems of every package folder under `root/providers`.
///
/// # Errors
///
/// Returns an error when `providers/` cannot be read.
pub fn check(root: &Path, inventory: &Inventory) -> Result<Vec<Problem>> {
    let providers = root.join("providers");
    let mut folders: Vec<PathBuf> = fs::read_dir(&providers)
        .with_context(|| format!("cannot read {}", providers.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    let mut problems = Vec::new();
    for folder in folders {
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        problems.extend(check_package(
            &folder,
            &format!("providers/{name}"),
            inventory,
        ));
    }
    Ok(problems)
}

/// The notice problems of one package folder, `rel` from the root.
fn check_package(folder: &Path, rel: &str, inventory: &Inventory) -> Vec<Problem> {
    let manifest_rel = format!("{rel}/provider.yaml");
    let Some((id, software)) = metadata(&folder.join("provider.yaml")) else {
        return vec![Problem::new(
            &manifest_rel,
            1,
            "cannot read `metadata.id` and `metadata.software`",
        )];
    };
    if id.starts_with("testing/") {
        return Vec::new();
    }
    let notice_rel = format!("{rel}/LICENSE-NOTICES.md");
    let Ok(text) = fs::read_to_string(folder.join("LICENSE-NOTICES.md")) else {
        return vec![Problem::new(
            &notice_rel,
            1,
            format!("`LICENSE-NOTICES.md` is missing; `{id}` is not a testing package"),
        )];
    };
    let doc = Markdown::parse(&text);
    let mut problems = Vec::new();
    let level_two: Vec<_> = doc
        .headings
        .iter()
        .filter(|heading| heading.level == 2)
        .collect();
    let mut next = 0;
    for heading in &level_two {
        if next < HEADINGS.len() && heading.text == HEADINGS[next] {
            next += 1;
        }
    }
    if next < HEADINGS.len() {
        problems.push(Problem::new(
            &notice_rel,
            1,
            format!(
                "headings must be `## {}` in this order; `## {}` is missing or out of order",
                HEADINGS.join("`, `## "),
                HEADINGS[next]
            ),
        ));
    }
    let Some(runtime) = level_two.iter().find(|heading| heading.text == "Runtime") else {
        return problems;
    };
    let end = level_two
        .iter()
        .find(|heading| heading.line > runtime.line)
        .map_or(usize::MAX, |heading| heading.line);
    let section: Vec<_> = doc
        .lines
        .iter()
        .filter(|line| line.number > runtime.line && line.number < end && line.prose.is_some())
        .collect();
    let spans: BTreeSet<String> = section
        .iter()
        .flat_map(|line| code_spans(&line.raw))
        .collect();
    let prose = section
        .iter()
        .map(|line| line.raw.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let prose = prose.split_whitespace().collect::<Vec<_>>().join(" ");
    let entries: Vec<_> = inventory
        .artifacts
        .iter()
        .filter(|(provider, _, _)| *provider == software)
        .collect();
    let licenses: BTreeSet<&str> = entries
        .iter()
        .map(|(_, license, _)| license.as_str())
        .collect();
    for license in licenses {
        if !spans.contains(license) {
            problems.push(Problem::new(
                &notice_rel,
                runtime.line,
                format!("`## Runtime` does not name the license `{license}` of `{INVENTORY}` as a code span"),
            ));
        }
    }
    let origins: BTreeSet<&str> = entries
        .iter()
        .map(|(_, _, origin)| origin.as_str())
        .collect();
    for (origin, sentence) in [("upstream", UPSTREAM), ("maintainer", MAINTAINER)] {
        let expected = origins.contains(origin);
        let written = prose.contains(sentence);
        if expected && !written {
            problems.push(Problem::new(
                &notice_rel,
                runtime.line,
                format!("`## Runtime` lacks \"{sentence}\", which origin `{origin}` in `{INVENTORY}` needs"),
            ));
        } else if written && !expected {
            problems.push(Problem::new(
                &notice_rel,
                runtime.line,
                format!("`## Runtime` says \"{sentence}\", but no `{software}` entry in `{INVENTORY}` has origin `{origin}`"),
            ));
        }
    }
    problems
}

/// `metadata.id` and `metadata.software` of a `provider.yaml`.
fn metadata(path: &Path) -> Option<(String, String)> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).ok()?;
    let metadata = value.get("metadata")?;
    let id = metadata.get("id")?.as_str()?.to_owned();
    let software = metadata.get("software")?.as_str()?.to_owned();
    Some((id, software))
}

#[cfg(test)]
mod tests {
    use super::super::fixture;

    #[test]
    fn a_missing_notice_is_one_problem_and_a_testing_package_needs_none() {
        let repo = fixture::repo();
        std::fs::remove_file(repo.path().join("providers/tool/LICENSE-NOTICES.md"))
            .expect("removed");
        assert_eq!(
            fixture::lines(repo.path()),
            vec![
                "providers/tool/LICENSE-NOTICES.md:1: `LICENSE-NOTICES.md` is missing; `official/tool` is not a testing package"
            ]
        );
    }

    #[test]
    fn an_unreadable_manifest_is_one_problem() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "providers/tool/provider.yaml",
            "metadata: {}\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec![
                "providers/tool/provider.yaml:1: cannot read `metadata.id` and `metadata.software`"
            ]
        );
    }

    #[test]
    fn headings_out_of_order_are_one_problem() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "providers/tool/LICENSE-NOTICES.md",
            "# N\n\n## Runtime\n\n`MIT`. Downloaded from the upstream release.\n\n## Package\n\n## Third party material in this package\n\nNone.\n",
        );
        let lines = fixture::lines(repo.path());
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("`## Runtime` is missing or out of order"),
            "{lines:?}"
        );
    }

    #[test]
    fn a_wrong_license_and_a_wrong_origin_are_problems() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "providers/tool/LICENSE-NOTICES.md",
            "# N\n\n## Package\n\nx\n\n## Runtime\n\nTool, `Apache-2.0`. Built by Stackly maintainers from upstream source.\n\n## Third party material in this package\n\nNone.\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec![
                "providers/tool/LICENSE-NOTICES.md:7: `## Runtime` does not name the license `MIT` of `runtimes/inventory.toml` as a code span",
                "providers/tool/LICENSE-NOTICES.md:7: `## Runtime` lacks \"Downloaded from the upstream release\", which origin `upstream` in `runtimes/inventory.toml` needs",
                "providers/tool/LICENSE-NOTICES.md:7: `## Runtime` says \"Built by Stackly maintainers from upstream source\", but no `tool` entry in `runtimes/inventory.toml` has origin `maintainer`",
            ]
        );
    }
}
