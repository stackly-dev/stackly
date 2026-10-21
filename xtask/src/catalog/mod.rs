//! `cargo xtask catalog build --out <dir>` and `cargo xtask catalog check
//! [--update [--amend]]` (spec 0017, AC-1): build the bundled catalog from
//! `providers/catalog.toml` and keep its revision ledger honest.
//!
//! `build` writes `<dir>/catalog.json` and `<dir>/packages/<id>/` copies of
//! each package's loader files. Both tasks print one line per problem and
//! exit 1. `cargo xtask build-app` (spec 0015) calls [`build`] in process.

mod ledger;
mod source;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, anyhow, bail};
use stackly_core::catalog::{
    AssembleOptions, AssembledCatalog, CATALOG_INDEX_FILE, SourceSpec, assemble, package_dir,
};
use stackly_core::provider::ValidatedPackage;

use ledger::{Entry, Ledger, Update};

/// The source file, relative to the repo root.
const SOURCE_FILE: &str = "providers/catalog.toml";
/// The ledger file, relative to the repo root.
const LEDGER_FILE: &str = "providers/catalog-revisions.json";

/// What a successful build or check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogReport {
    /// The catalog revision.
    pub revision: u64,
    /// The content digest.
    pub content_sha256: String,
    /// How many provider entries.
    pub providers: usize,
    /// How many planned entries.
    pub planned: usize,
}

/// Every problem a build or check found, one lowercase line each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problems(pub Vec<String>);

impl std::fmt::Display for Problems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

impl std::error::Error for Problems {}

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    let outcome = match args {
        ["build", "--out", out] => {
            let out = std::env::current_dir()
                .context("cannot read the current directory")?
                .join(out);
            build(root, &out).map(|report| {
                println!(
                    "catalog: revision {}, {} provider(s), {} planned -> {}",
                    report.revision,
                    report.providers,
                    report.planned,
                    out.display()
                );
            })
        }
        ["check"] => check(root, None).map(|report| print_ok(&report)),
        ["check", "--update"] => check(root, Some(false)).map(|report| print_ok(&report)),
        ["check", "--update", "--amend"] => check(root, Some(true)).map(|report| print_ok(&report)),
        _ => bail!(
            "unknown catalog task `{}`; see `cargo xtask help`",
            args.join(" ")
        ),
    };
    match outcome {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(err) => match err.downcast_ref::<Problems>() {
            Some(problems) => {
                for line in &problems.0 {
                    eprintln!("{line}");
                }
                eprintln!("catalog: {} problem(s)", problems.0.len());
                Ok(ExitCode::FAILURE)
            }
            None => Err(err),
        },
    }
}

fn print_ok(report: &CatalogReport) {
    println!(
        "ok catalog revision {} {}",
        report.revision, report.content_sha256
    );
}

/// Builds the bundled catalog into `out`: checks the source, the packages,
/// and the ledger, writes a sibling temporary folder, then renames it to
/// `out`. An existing `out` is replaced only when it holds a
/// `catalog.json` from an earlier build.
///
/// # Errors
///
/// Fails with [`Problems`] for every rule the source, a package, or the
/// ledger breaks, and with an IO error when the folder cannot be written.
pub fn build(root: &Path, out: &Path) -> Result<CatalogReport> {
    let (catalog, report) = prepare(root, true)?;
    write_catalog(&catalog, out)?;
    Ok(report)
}

/// Checks the catalog without writing it; with `update`, records the
/// current revision in the ledger (`Some(true)` also allows `--amend`).
fn check(root: &Path, update: Option<bool>) -> Result<CatalogReport> {
    let (catalog, report) = prepare(root, update.is_none())?;
    if let Some(amend) = update {
        let path = root.join(LEDGER_FILE);
        let mut ledger = read_ledger(&path)?;
        let on_main = ledger_on_main(root)?;
        let entry = ledger_entry(&catalog);
        let outcome = ledger
            .update(entry, amend, &on_main)
            .map_err(|err| Problems(vec![err.to_string()]))?;
        if outcome != Update::Unchanged {
            std::fs::write(&path, ledger.to_pretty_json()?)
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
    }
    Ok(report)
}

/// Reads the source and packages and assembles the catalog; with
/// `with_ledger`, also checks that its revision is recorded.
fn prepare(root: &Path, with_ledger: bool) -> Result<(AssembledCatalog, CatalogReport)> {
    let source_path = root.join(SOURCE_FILE);
    let text = std::fs::read_to_string(&source_path)
        .with_context(|| format!("cannot read {}", source_path.display()))?;
    let spec = source::parse(&text)?;
    let (packages, mut problems) = read_packages(root, &spec)?;
    let options = AssembleOptions {
        allow_testing: false,
        not_after: Some(jiff::Timestamp::now()),
    };
    let catalog = match assemble(&spec, &packages, options) {
        Ok(catalog) => Some(catalog),
        Err(found) => {
            problems.extend(found);
            None
        }
    };
    if let (Some(catalog), true) = (&catalog, with_ledger) {
        let ledger = read_ledger(&root.join(LEDGER_FILE))?;
        if let Err(err) = ledger.check(&ledger_entry(catalog)) {
            problems.push(err.to_string());
        }
    }
    match catalog {
        Some(catalog) if problems.is_empty() => {
            let report = CatalogReport {
                revision: catalog.index.revision,
                content_sha256: catalog.index.content_sha256.clone(),
                providers: catalog.index.providers.len(),
                planned: catalog.index.planned.len(),
            };
            Ok((catalog, report))
        }
        _ => Err(Problems(problems).into()),
    }
}

fn ledger_entry(catalog: &AssembledCatalog) -> Entry {
    Entry {
        revision: catalog.index.revision,
        published_at: catalog.index.published_at,
        content_sha256: catalog.index.content_sha256.clone(),
    }
}

fn read_ledger(path: &Path) -> Result<Ledger> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ledger::parse(&text)
}

/// The ledger as committed on `main`, or an empty one when `main` has no
/// ledger yet.
fn ledger_on_main(root: &Path) -> Result<Ledger> {
    let output = Command::new("git")
        .args(["show", &format!("main:{LEDGER_FILE}")])
        .current_dir(root)
        .output()
        .context("cannot run git show")?;
    if !output.status.success() {
        return Ok(Ledger::default());
    }
    Ledger::parse(&String::from_utf8_lossy(&output.stdout))
}

/// Reads and validates the package of every provider entry from
/// `providers/<name>/`, keyed by the entry ID. A package that does not read
/// or validate becomes one problem line per issue.
fn read_packages(
    root: &Path,
    spec: &SourceSpec,
) -> Result<(BTreeMap<String, ValidatedPackage>, Vec<String>)> {
    let mut packages = BTreeMap::new();
    let mut problems = Vec::new();
    for provider in &spec.providers {
        let Some((_, name)) = provider.id.split_once('/') else {
            continue;
        };
        let dir = root.join("providers").join(name);
        match read_and_validate(&dir)? {
            Ok(package) => {
                packages.insert(provider.id.clone(), package);
            }
            Err(err) => problems.extend(
                err.issues
                    .iter()
                    .map(|issue| format!("{}: {issue}", provider.id)),
            ),
        }
    }
    Ok((packages, problems))
}

#[cfg(target_os = "macos")]
fn read_and_validate(
    dir: &Path,
) -> Result<Result<ValidatedPackage, stackly_core::provider::PackageError>> {
    use stackly_core::provider::{NameCheck, read_package};
    use stackly_platform_macos::MacosFilesystem;

    Ok(read_package(&MacosFilesystem, dir)
        .and_then(|files| ValidatedPackage::from_files(files, NameCheck::FolderName)))
}

#[cfg(not(target_os = "macos"))]
fn read_and_validate(
    _dir: &Path,
) -> Result<Result<ValidatedPackage, stackly_core::provider::PackageError>> {
    bail!("catalog build reads provider packages on macOS only, through MacosFilesystem")
}

/// Writes the index and package copies to a temporary folder next to
/// `out`, then renames it to `out`.
fn write_catalog(catalog: &AssembledCatalog, out: &Path) -> Result<()> {
    let name = out
        .file_name()
        .ok_or_else(|| anyhow!("{} has no folder name", out.display()))?
        .to_string_lossy()
        .into_owned();
    let parent = out
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent folder", out.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("cannot create {}", parent.display()))?;
    if out.exists() && !out.join(CATALOG_INDEX_FILE).is_file() {
        bail!(
            "refusing to replace {}: it is not a catalog folder (no {CATALOG_INDEX_FILE})",
            out.display()
        );
    }
    let staging = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("cannot clear {}", staging.display()))?;
    }
    let written = write_into(catalog, &staging);
    if let Err(err) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }
    if out.exists() {
        std::fs::remove_dir_all(out)
            .with_context(|| format!("cannot replace {}", out.display()))?;
    }
    std::fs::rename(&staging, out)
        .with_context(|| format!("cannot move the catalog to {}", out.display()))?;
    Ok(())
}

fn write_into(catalog: &AssembledCatalog, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    write_file(
        &dir.join(CATALOG_INDEX_FILE),
        catalog.index.to_pretty_json().as_bytes(),
    )?;
    for (id, files) in &catalog.packages {
        let package = dir.join(package_dir(id));
        for (path, bytes) in files.iter() {
            write_file(&package.join(path), bytes)?;
        }
    }
    Ok(())
}

fn write_file(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use std::fs;

    use stackly_testkit::TestRoot;

    use super::*;

    const SOURCE: &str = r#"schema = "stackly.catalog-source/v1"
revision = 1
published_at = 2026-10-19T14:05:00Z

[[provider]]
id = "official/nats"
tile = "green"

[[planned]]
id = "official/mysql"
display_name = "MySQL"
category = "database"
description = "Relational server."
"#;

    /// A repo copy in a test root: `providers/nats` and a catalog source.
    fn repo(source: &str) -> TestRoot {
        let root = TestRoot::new().unwrap();
        let providers = root.path().join("providers");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../providers/nats"),
            &providers.join("nats"),
        );
        fs::write(providers.join("catalog.toml"), source).unwrap();
        fs::write(
            providers.join("catalog-revisions.json"),
            "{ \"revisions\": [] }\n",
        )
        .unwrap();
        root
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn problems(err: anyhow::Error) -> Vec<String> {
        err.downcast::<Problems>().unwrap().0
    }

    /// Every file under a folder with its bytes, by relative path.
    fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for entry in fs::read_dir(&next).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    stack.push(entry.path());
                } else {
                    let path = entry.path();
                    let relative = path.strip_prefix(dir).unwrap().display().to_string();
                    files.insert(relative, fs::read(&path).unwrap());
                }
            }
        }
        files
    }

    #[test]
    fn a_revision_missing_from_the_ledger_stops_the_build() {
        let root = repo(SOURCE);

        let found = problems(build(root.path(), &root.path().join("out")).unwrap_err());

        assert_eq!(
            found,
            [
                "catalog content changed: bump revision and published_at in providers/catalog.toml, then run cargo xtask catalog check --update"
            ]
        );
        assert!(!root.path().join("out").exists());
    }

    #[test]
    fn update_records_the_revision_and_the_build_writes_the_same_bytes_twice() {
        let root = repo(SOURCE);
        let report = check(root.path(), Some(false)).unwrap();
        let out = root.path().join("out");

        assert_eq!(build(root.path(), &out).unwrap(), report);
        let first = tree(&out);
        build(root.path(), &out).unwrap();

        assert_eq!(tree(&out), first);
        assert_eq!(
            first.keys().cloned().collect::<Vec<_>>(),
            [
                "catalog.json",
                "packages/official/nats/provider.yaml",
                "packages/official/nats/schemas/settings.schema.json",
                "packages/official/nats/templates/nats.conf.tmpl",
                "packages/official/nats/versions.json",
            ]
        );
        assert_eq!(report.providers, 1);
        assert_eq!(report.planned, 1);
        assert!(
            check(root.path(), None).is_ok(),
            "the recorded revision checks"
        );
    }

    #[test]
    fn changed_content_under_a_recorded_revision_is_refused() {
        let root = repo(SOURCE);
        check(root.path(), Some(false)).unwrap();
        fs::write(
            root.path().join("providers/catalog.toml"),
            SOURCE.replace("Relational server.", "Relational database server."),
        )
        .unwrap();

        assert_eq!(
            check(root.path(), None)
                .unwrap_err()
                .downcast::<Problems>()
                .unwrap()
                .0
                .len(),
            1
        );
        assert!(check(root.path(), Some(false)).is_err());
    }

    #[test]
    fn a_folder_that_is_not_a_catalog_is_never_replaced() {
        let root = repo(SOURCE);
        check(root.path(), Some(false)).unwrap();
        let out = root.path().join("out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("keep.txt"), "mine").unwrap();

        let err = build(root.path(), &out).unwrap_err();

        assert!(err.to_string().contains("is not a catalog folder"), "{err}");
        assert_eq!(fs::read_to_string(out.join("keep.txt")).unwrap(), "mine");
    }

    #[test]
    fn testing_packages_missing_folders_and_future_times_are_refused() {
        let testing = SOURCE.replace("official/nats", "testing/fake");
        let missing = SOURCE.replace("official/nats", "official/postgresql");
        let future = SOURCE.replace("2026-10-19T14:05:00Z", "2999-01-01T00:00:00Z");

        let testing =
            problems(build(repo(&testing).path(), Path::new("/nowhere/out")).unwrap_err());
        let missing =
            problems(build(repo(&missing).path(), Path::new("/nowhere/out")).unwrap_err());
        let future = problems(build(repo(&future).path(), Path::new("/nowhere/out")).unwrap_err());

        assert!(
            testing
                .iter()
                .any(|line| line.contains("testing/fake is not in the official namespace")),
            "{testing:?}"
        );
        assert!(
            missing
                .iter()
                .any(|line| line.starts_with("official/postgresql: ")),
            "{missing:?}"
        );
        assert!(
            future.iter().any(|line| line.contains("is in the future")),
            "{future:?}"
        );
    }
}
