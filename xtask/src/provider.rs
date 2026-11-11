//! `cargo xtask provider check [<dir>...]`: checks provider packages with
//! `check_package` (spec 0036, AC-4), the validator the daemon and import
//! share, through `MacosFilesystem`.
//!
//! Every publishable package prints `ok <id> <revision> <manifest_sha256>`;
//! every error finding (`package`, `authoring`, such as `init_port_issues`,
//! or `scenarios`) prints one `<folder>: <file> <location>: <code>:
//! <message>` line and no `ok` line, every warning prints the same line
//! after `warning: `, and the task exits 1 when a package is not
//! publishable. Without folders it checks every folder in `providers/`,
//! where a warning fails the check too, and every `[[provider]]` of
//! `providers/catalog.toml` must have a corpus entry with its digest.
//!
//! `cargo xtask provider freeze <dir>` writes that entry ([`freeze`]).

#[cfg(target_os = "macos")]
mod freeze;

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, bail};

mod harness;
mod scaffold;

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["check", dirs @ ..] => check(root, dirs),
        ["freeze", dir] => freeze_task(root, dir),
        ["new", rest @ ..] => scaffold::run(root, rest),
        ["test", rest @ ..] => harness::run(root, rest),
        _ => bail!(
            "unknown provider task `{}`; see `cargo xtask help`",
            args.join(" ")
        ),
    }
}

#[cfg(target_os = "macos")]
fn check(root: &Path, dirs: &[&str]) -> Result<ExitCode> {
    use anyhow::Context;

    let packages = if dirs.is_empty() {
        repo_packages(root)?
    } else {
        let cwd = std::env::current_dir().context("cannot read the current directory")?;
        dirs.iter().map(|dir| cwd.join(dir)).collect()
    };
    let mut report = macos::check_packages(&packages, dirs.is_empty());
    if dirs.is_empty() {
        not_frozen(root, &mut report)?;
    }
    for line in &report.lines {
        println!("{line}");
    }
    if report.problems == 0 {
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "provider check: {} problem(s) in {} of {} package(s)",
        report.problems,
        report.failed,
        packages.len()
    );
    Ok(ExitCode::FAILURE)
}

/// Adds one `not frozen` line, and one problem, for every `[[provider]]` of
/// the catalog whose package in `providers/` is publishable but has no
/// corpus entry with its digest (spec 0036, AC-4).
#[cfg(target_os = "macos")]
fn not_frozen(root: &Path, report: &mut macos::Report) -> Result<()> {
    use stackly_core::provider::{NameCheck, check_package};
    use stackly_platform_macos::MacosFilesystem;

    let corpus = root.join(freeze::CORPUS_DIR);
    for id in freeze::catalog_providers(root)? {
        let folder = id.rsplit('/').next().unwrap_or(&id).to_owned();
        let check = check_package(
            &MacosFilesystem,
            &root.join("providers").join(&folder),
            NameCheck::FolderName,
        );
        let Some(package) = check.package.as_ref().filter(|_| check.publishable()) else {
            continue;
        };
        if !freeze::is_frozen(&corpus, package) {
            report.lines.push(format!(
                "{folder}: not frozen: run cargo xtask provider freeze providers/{folder}"
            ));
            report.problems += 1;
            report.failed += 1;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn freeze_task(root: &Path, dir: &str) -> Result<ExitCode> {
    use anyhow::Context;

    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    match freeze::freeze(&root.join(freeze::CORPUS_DIR), &cwd.join(dir)) {
        Ok(freeze::Frozen::Written(entry)) => {
            println!("frozen {}", entry.display());
            Ok(ExitCode::SUCCESS)
        }
        Ok(freeze::Frozen::Already(entry)) => {
            println!("already frozen {}", entry.display());
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            eprintln!("provider freeze: {err:#}");
            Ok(ExitCode::FAILURE)
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn freeze_task(_root: &Path, _dir: &str) -> Result<ExitCode> {
    bail!("provider freeze runs on macOS only, because it reads packages through MacosFilesystem")
}

#[cfg(not(target_os = "macos"))]
fn check(_root: &Path, _dirs: &[&str]) -> Result<ExitCode> {
    bail!("provider check runs on macOS only, because it reads packages through MacosFilesystem")
}

/// Every package folder in `providers/`, sorted by name.
#[cfg(target_os = "macos")]
fn repo_packages(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    use anyhow::Context;

    let dir = root.join("providers");
    let mut packages = Vec::new();
    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        if entry.file_type()?.is_dir() {
            packages.push(entry.path());
        }
    }
    if packages.is_empty() {
        bail!("no provider packages in {}", dir.display());
    }
    packages.sort();
    Ok(packages)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};

    use stackly_core::provider::{Finding, NameCheck, Severity, check_package};
    use stackly_platform_macos::MacosFilesystem;

    /// What `provider check` prints, and how many problems it found.
    #[derive(Debug, Default)]
    pub struct Report {
        /// One `ok` line per publishable package, one line per error, and
        /// one `warning: ` line per warning, in package order.
        pub lines: Vec<String>,
        /// Every error across all packages, plus every warning when
        /// warnings fail the check.
        pub problems: usize,
        /// Packages with at least one problem.
        pub failed: usize,
    }

    /// Checks each package folder with `check_package` through
    /// `MacosFilesystem` and `NameCheck::FolderName` (spec 0036, AC-4).
    /// With `warnings_fail`, which the check of `providers/` sets, a
    /// warning is a problem too, so a repo package never ships a
    /// deprecated item.
    pub fn check_packages(packages: &[PathBuf], warnings_fail: bool) -> Report {
        let mut report = Report::default();
        for dir in packages {
            let check = check_package(&MacosFilesystem, dir, NameCheck::FolderName);
            let folder = folder_name(dir);
            let mut problems = 0;
            let mut warnings = Vec::new();
            for finding in &check.findings {
                match finding.severity {
                    Severity::Error => {
                        problems += 1;
                        report.lines.push(line(&folder, finding));
                    }
                    Severity::Warning => warnings.push(finding),
                }
            }
            if problems == 0
                && let Some(package) = &check.package
            {
                let key = package.key();
                report.lines.push(format!(
                    "ok {} {} {}",
                    key.provider(),
                    key.revision(),
                    key.manifest_sha256()
                ));
            }
            for finding in warnings {
                report
                    .lines
                    .push(format!("warning: {}", line(&folder, finding)));
                if warnings_fail {
                    problems += 1;
                }
            }
            if problems > 0 {
                report.failed += 1;
                report.problems += problems;
            }
        }
        report
    }

    /// `<folder>: <file> <location>: <code>: <message>`, the line format
    /// of spec 0009 AC-14.
    fn line(folder: &str, finding: &Finding) -> String {
        format!("{folder}: {}", finding.issue)
    }

    /// The folder name a problem line starts with, or the whole path when
    /// it has none.
    fn folder_name(dir: &Path) -> String {
        dir.file_name().map_or_else(
            || dir.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_missing_folder_is_one_problem_named_after_it() {
            let root = stackly_testkit::TestRoot::new().unwrap();

            let report = check_packages(&[root.path().join("nowhere")], false);

            assert_eq!(report.problems, report.lines.len());
            assert!(report.problems >= 1);
            assert_eq!(report.failed, 1);
            assert!(
                report.lines[0].starts_with("nowhere: "),
                "{:?}",
                report.lines
            );
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod not_frozen_tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_catalog_provider_without_a_corpus_entry_is_one_not_frozen_problem() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let providers = root.path().join("providers");
        fs::create_dir_all(providers.join("nats")).unwrap();
        for entry in walk(&repo.join("providers/nats")) {
            let relative = entry.strip_prefix(repo.join("providers/nats")).unwrap();
            let target = providers.join("nats").join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(&entry, &target).unwrap();
        }
        fs::write(
            providers.join("catalog.toml"),
            "[[provider]]\nid = \"official/nats\"\n",
        )
        .unwrap();
        let mut report = macos::Report::default();

        not_frozen(root.path(), &mut report).unwrap();

        assert_eq!(
            report.lines,
            ["nats: not frozen: run cargo xtask provider freeze providers/nats"]
        );
        assert_eq!((report.problems, report.failed), (1, 1));

        freeze::freeze(
            &root.path().join(freeze::CORPUS_DIR),
            &providers.join("nats"),
        )
        .unwrap();
        let mut report = macos::Report::default();
        not_frozen(root.path(), &mut report).unwrap();
        assert!(report.lines.is_empty(), "{:?}", report.lines);
    }

    fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        for entry in fs::read_dir(dir).expect("the package folder lists") {
            let path = entry.expect("a package entry reads").path();
            if path.is_dir() {
                files.extend(walk(&path));
            } else {
                files.push(path);
            }
        }
        files
    }
}
