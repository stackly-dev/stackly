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
//! where a warning fails the check too.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{Result, bail};

mod scaffold;

pub fn run(root: &Path, args: &[&str]) -> Result<ExitCode> {
    match args {
        ["check", dirs @ ..] => check(root, dirs),
        ["new", rest @ ..] => scaffold::run(root, rest),
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
    let report = macos::check_packages(&packages, dirs.is_empty());
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
