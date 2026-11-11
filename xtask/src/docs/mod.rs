//! `cargo xtask docs check` (spec 0038, AC-9): checks the public docs so
//! they cannot go stale without anyone noticing, because the repo has no CI.
//!
//! The check reads the public doc set (`README.md`, `CONTRIBUTING.md`,
//! `CODE_OF_CONDUCT.md`, `SECURITY.md`, `GOVERNANCE.md`, everything in
//! `.github/`, `docs/providers/**/*.md`, `providers/*/README.md`, and
//! `providers/*/LICENSE-NOTICES.md`), writes nothing, opens no network, and
//! never runs a command it finds in a doc. It prints `ok docs <n>` or one
//! `path:line: message` line per problem, sorted by path and line, and exits
//! 0 clean, 1 with problems, 2 when an input it needs cannot be read.

mod commands;
mod forms;
mod links;
mod markdown;
mod notices;
mod schema;
mod text;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

pub use commands::Tables;
use markdown::Markdown;

/// The private vulnerability reporting URL the issue chooser links to.
pub const ADVISORY_URL: &str = "https://github.com/stackly-dev/stackly/security/advisories/new";

/// The labels an issue form may use (spec 0038, *Label triage*).
pub const TRIAGE_LABELS: [&str; 5] = [
    "needs-triage",
    "core",
    "provider",
    "upstream",
    "configuration",
];

/// Runs `cargo xtask docs <args>`.
///
/// # Errors
///
/// Returns an error for an unknown docs task.
pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["check"] => Ok(check_command(root)),
        _ => bail!(
            "unknown docs task `{}`; see `cargo xtask help`",
            args.join(" ")
        ),
    }
}

/// `cargo xtask docs check` on the repo at `root`.
fn check_command(root: &Path) -> ExitCode {
    let tables = match Tables::load(root) {
        Ok(tables) => tables,
        Err(err) => {
            eprintln!("error: {err:#}");
            return ExitCode::from(2);
        }
    };
    match check(root, &tables) {
        Ok(outcome) => {
            if outcome.problems.is_empty() {
                println!("ok docs {}", outcome.files);
                ExitCode::SUCCESS
            } else {
                for problem in &outcome.problems {
                    println!("{problem}");
                }
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

/// One problem in one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Problem {
    /// The file, relative to the repo root, with `/` separators.
    pub path: String,
    /// The 1 based line.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

impl Problem {
    /// A problem at `path:line`.
    #[must_use]
    pub fn new(path: &str, line: usize, message: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.path, self.line, self.message)
    }
}

/// What one check of the doc set found.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// How many public doc files were read.
    pub files: usize,
    /// Every problem, sorted by path and line.
    pub problems: Vec<Problem>,
}

/// How a file of the public doc set is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A Markdown file: every check.
    Markdown,
    /// `CODE_OF_CONDUCT.md`: only links and `[INSERT`, because its text
    /// is never changed.
    CodeOfConduct,
    /// A YAML file in `.github/`: links and placeholders on every string
    /// value, plus the issue form rules in `.github/ISSUE_TEMPLATE/`.
    Yaml,
}

/// One file of the public doc set.
#[derive(Debug, Clone)]
struct DocFile {
    /// The path relative to the root, with `/` separators.
    rel: String,
    /// The absolute path.
    abs: PathBuf,
    /// How it is checked.
    kind: Kind,
}

/// Checks the public doc set under `root` against the command `tables`.
///
/// # Errors
///
/// Returns an error when `providers/`, `schemas/provider/`, or
/// `runtimes/inventory.toml` cannot be read, which the command turns into
/// exit 2.
pub fn check(root: &Path, tables: &Tables) -> Result<Outcome> {
    let inventory = notices::Inventory::load(root)?;
    let schemas = root.join("schemas/provider");
    if !schemas.is_dir() {
        bail!("cannot read {}", schemas.display());
    }
    let files = doc_set(root)?;
    let mut links = links::Checker::new(root);
    let mut problems = Vec::new();
    for file in &files {
        let text = fs::read_to_string(&file.abs)
            .with_context(|| format!("cannot read {}", file.abs.display()))?;
        match file.kind {
            Kind::Markdown | Kind::CodeOfConduct => {
                let doc = Markdown::parse(&text);
                problems.extend(links.check(&file.rel, &file.abs, &doc));
                if file.kind == Kind::CodeOfConduct {
                    problems.extend(text::insert_markers(&file.rel, &doc));
                } else {
                    problems.extend(text::placeholders(&file.rel, &doc));
                    problems.extend(text::lint(&file.rel, &doc));
                    problems.extend(tables.check(&file.rel, &doc));
                }
            }
            Kind::Yaml => {
                problems.extend(forms::check_strings(
                    &file.rel, &file.abs, &text, &mut links,
                ));
                if file.rel.starts_with(".github/ISSUE_TEMPLATE/") {
                    problems.extend(forms::check_form(&file.rel, &text));
                }
            }
        }
    }
    problems.extend(schema::check(root)?);
    problems.extend(notices::check(root, &inventory)?);
    problems.extend(text::approval_statement(root)?);
    problems.sort();
    problems.dedup();
    Ok(Outcome {
        files: files.len(),
        problems,
    })
}

/// Every file of the public doc set that exists under `root`, sorted.
fn doc_set(root: &Path) -> Result<Vec<DocFile>> {
    let mut files = Vec::new();
    for name in [
        "README.md",
        "CONTRIBUTING.md",
        "CODE_OF_CONDUCT.md",
        "SECURITY.md",
        "GOVERNANCE.md",
    ] {
        let abs = root.join(name);
        if abs.is_file() {
            let kind = if name == "CODE_OF_CONDUCT.md" {
                Kind::CodeOfConduct
            } else {
                Kind::Markdown
            };
            files.push(DocFile {
                rel: name.to_owned(),
                abs,
                kind,
            });
        }
    }
    for abs in walk(&root.join(".github"))? {
        let kind = match abs.extension().and_then(|ext| ext.to_str()) {
            Some("md") => Kind::Markdown,
            Some("yml" | "yaml") => Kind::Yaml,
            _ => continue,
        };
        files.push(DocFile {
            rel: relative(root, &abs),
            abs,
            kind,
        });
    }
    for abs in walk(&root.join("docs/providers"))? {
        if abs.extension().is_some_and(|ext| ext == "md") {
            files.push(DocFile {
                rel: relative(root, &abs),
                abs,
                kind: Kind::Markdown,
            });
        }
    }
    let providers = root.join("providers");
    let entries =
        fs::read_dir(&providers).with_context(|| format!("cannot read {}", providers.display()))?;
    let mut folders: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    for folder in folders {
        for name in ["README.md", "LICENSE-NOTICES.md"] {
            let abs = folder.join(name);
            if abs.is_file() {
                files.push(DocFile {
                    rel: relative(root, &abs),
                    abs,
                    kind: Kind::Markdown,
                });
            }
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}

/// Every file under `dir`, recursively and sorted; nothing when `dir` is
/// missing.
fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    if !dir.is_dir() {
        return Ok(found);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries =
            fs::read_dir(&current).with_context(|| format!("cannot read {}", current.display()))?;
        for entry in entries {
            let path = entry
                .with_context(|| format!("cannot read {}", current.display()))?
                .path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// `path` relative to `root`, with `/` separators.
fn relative(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
pub(crate) mod fixture {
    //! A clean fixture repo for the docs check tests: one official package
    //! with its notice, one testing package, an inventory, and the three
    //! provider schemas with a reference page that covers each.

    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{Outcome, Tables, check};

    /// A clean fixture repo.
    pub fn repo() -> TempDir {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path();
        write(
            root,
            "README.md",
            "# Fixture\n\nSee [the guide](CONTRIBUTING.md#set-up).\n",
        );
        write(
            root,
            "CONTRIBUTING.md",
            "# Contributing\n\n## Set up\n\n```bash\ncargo xtask codegen --check\nstackly daemon status --json\n```\n",
        );
        write(
            root,
            "providers/tool/provider.yaml",
            "metadata:\n  id: official/tool\n  software: tool\n",
        );
        write(
            root,
            "providers/tool/LICENSE-NOTICES.md",
            "# Notices\n\n## Package\n\nMIT OR Apache-2.0.\n\n## Runtime\n\nTool, `MIT`. Downloaded from the upstream release.\n\n## Third party material in this package\n\nNone.\n",
        );
        write(
            root,
            "providers/fake/provider.yaml",
            "metadata:\n  id: testing/fake\n  software: fake\n",
        );
        write(
            root,
            "runtimes/inventory.toml",
            "[[artifact]]\nprovider = \"tool\"\norigin = \"upstream\"\nlicense = \"MIT\"\n",
        );
        for (schema, page) in super::schema::PAIRS {
            write(
                root,
                schema,
                r#"{"properties":{"apiVersion":{"const":"stackly/v1alpha1"}}}"#,
            );
            write(
                root,
                page,
                "# Reference\n\n`apiVersion` is always `stackly/v1alpha1`.\n",
            );
        }
        dir
    }

    /// Writes `text` to `rel` under `root`, creating folders.
    pub fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("a file has a parent")).expect("folders");
        fs::write(path, text).expect("the fixture file is written");
    }

    /// The command tables the fixtures use.
    pub fn tables() -> Tables {
        Tables::from_parts(
            "  codegen --check      Check\n  provider check [<dir>...]\n",
            &[
                "stackly",
                "stackly-daemon",
                "stackly-daemon-status",
                "stackly-provider",
                "stackly-provider-list",
            ],
        )
    }

    /// Runs the check on `root`.
    pub fn run(root: &Path) -> Outcome {
        check(root, &tables()).expect("the fixture inputs are readable")
    }

    /// The problems on `root`, as printed.
    pub fn lines(root: &Path) -> Vec<String> {
        run(root).problems.iter().map(ToString::to_string).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::fixture;

    #[test]
    fn the_clean_fixture_has_no_problems_and_counts_its_files() {
        let repo = fixture::repo();
        let outcome = fixture::run(repo.path());
        assert_eq!(outcome.problems, Vec::new());
        assert_eq!(outcome.files, 6);
    }

    #[test]
    fn problems_are_sorted_by_path_then_line() {
        let repo = fixture::repo();
        fixture::write(repo.path(), "SECURITY.md", "# S\n\nTBD\n\nTODO\n");
        fixture::write(repo.path(), "GOVERNANCE.md", "# G\n\nFIXME\n");
        let lines = fixture::lines(repo.path());
        assert_eq!(
            lines,
            vec![
                "GOVERNANCE.md:3: placeholder `FIXME`",
                "SECURITY.md:3: placeholder `TBD`",
                "SECURITY.md:5: placeholder `TODO`",
            ]
        );
    }

    #[test]
    fn a_missing_inventory_is_an_input_error_not_a_problem() {
        let repo = fixture::repo();
        std::fs::remove_file(repo.path().join("runtimes/inventory.toml")).expect("removed");
        assert!(super::check(repo.path(), &fixture::tables()).is_err());
    }

    #[test]
    fn a_missing_schemas_folder_is_an_input_error() {
        let repo = fixture::repo();
        std::fs::remove_dir_all(repo.path().join("schemas")).expect("removed");
        assert!(super::check(repo.path(), &fixture::tables()).is_err());
    }
}
