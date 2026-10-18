//! `cargo xtask provider check [<dir>...]`: reads and validates provider
//! packages with the engine of spec 0009, through `MacosFilesystem`, the
//! way the catalog will read them.
//!
//! Every valid package prints `ok <id> <revision> <manifest_sha256>`;
//! every problem prints one `<folder>: <file> <location>: <code>: <message>`
//! line, and the task exits 1.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, bail};

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["check", dirs @ ..] => check(root, dirs),
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
    let report = macos::check_packages(&packages);
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

    use stackly_core::provider::{NameCheck, PackageError, ValidatedPackage, read_package};
    use stackly_platform_macos::MacosFilesystem;

    /// What `provider check` prints, and how many problems it found.
    #[derive(Debug, Default)]
    pub struct Report {
        /// One `ok` line per valid package and one line per problem, in
        /// package order.
        pub lines: Vec<String>,
        /// Every problem across all packages.
        pub problems: usize,
        /// Packages with at least one problem.
        pub failed: usize,
    }

    /// Reads each package folder through `MacosFilesystem` and validates it
    /// with `NameCheck::FolderName`.
    pub fn check_packages(packages: &[PathBuf]) -> Report {
        let mut report = Report::default();
        for dir in packages {
            match read_and_validate(dir) {
                Ok(package) => {
                    let key = package.key();
                    report.lines.push(format!(
                        "ok {} {} {}",
                        key.provider(),
                        key.revision(),
                        key.manifest_sha256()
                    ));
                }
                Err(err) => {
                    report.failed += 1;
                    report.problems += err.issues.len();
                    let folder = folder_name(dir);
                    report
                        .lines
                        .extend(err.issues.iter().map(|issue| format!("{folder}: {issue}")));
                }
            }
        }
        report
    }

    fn read_and_validate(dir: &Path) -> Result<ValidatedPackage, PackageError> {
        let files = read_package(&MacosFilesystem, dir)?;
        ValidatedPackage::from_files(files, NameCheck::FolderName)
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

            let report = check_packages(&[root.path().join("nowhere")]);

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
