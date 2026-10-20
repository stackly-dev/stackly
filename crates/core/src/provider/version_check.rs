//! The manifest `runtime.versionCheck` block (spec 0009, fase 02 field of
//! spec 0018): the arguments the installer runs the runtime executable
//! with once, and the pattern that reads the version from its output.
//!
//! The block is not part of any plan, but it lives in `provider.yaml`, so
//! it counts toward the manifest digest. Unlike a settings pattern, the
//! pattern is not anchored: it is searched for in each line of the output
//! ([`VersionCheck::read_version`]).

use std::time::Duration;

use regex::{Regex, RegexBuilder};

use super::interpolate::check_literal;
use super::issue::{FileIssues, IssueCode};
use super::raw::RawVersionCheck;
use super::settings::{PATTERN_MAX, PATTERN_SIZE_LIMIT};

/// Most `args` one version check passes.
pub const VERSION_CHECK_ARGS_MAX: usize = 8;
/// The time limit when `timeoutSeconds` is left out: 15 seconds.
pub const VERSION_CHECK_TIMEOUT_DEFAULT: Duration = Duration::from_secs(15);
/// The longest `timeoutSeconds`: 60.
pub const VERSION_CHECK_TIMEOUT_MAX_SECONDS: u64 = 60;

/// A checked `runtime.versionCheck`.
#[derive(Debug, Clone)]
pub struct VersionCheck {
    args: Vec<String>,
    pattern: Regex,
    timeout: Duration,
}

impl VersionCheck {
    /// Returns the literal arguments, in order.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Returns the pattern as written in the manifest.
    #[must_use]
    pub fn pattern(&self) -> &str {
        self.pattern.as_str()
    }

    /// Returns the time limit of the check.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Searches every line of `stdout`, then every line of `stderr`, with
    /// the trailing `\n` and `\r` removed, and returns the capture group of
    /// the first line the pattern matches (spec 0018, *Validasi binary*
    /// step 8).
    #[must_use]
    pub fn read_version<'a>(&self, stdout: &'a str, stderr: &'a str) -> Option<&'a str> {
        stdout
            .lines()
            .chain(stderr.lines())
            .map(|line| line.trim_end_matches(['\r', '\n']))
            .find_map(|line| {
                self.pattern
                    .captures(line)
                    .and_then(|captures| captures.get(1))
                    .map(|found| found.as_str())
            })
    }
}

impl PartialEq for VersionCheck {
    fn eq(&self, other: &Self) -> bool {
        self.args == other.args
            && self.pattern.as_str() == other.pattern.as_str()
            && self.timeout == other.timeout
    }
}

impl Eq for VersionCheck {}

/// Checks the block: at most [`VERSION_CHECK_ARGS_MAX`] literal arguments,
/// a pattern within the settings pattern limits with exactly one capture
/// group, and a timeout of 1 to 60 seconds. Returns `None` when the block
/// is absent or has an issue.
pub(crate) fn check(
    raw: Option<&RawVersionCheck>,
    issues: &mut FileIssues<'_>,
) -> Option<VersionCheck> {
    let raw = raw?;
    let path = "runtime.versionCheck";
    let start = issues.count();
    if raw.args.len() > VERSION_CHECK_ARGS_MAX {
        issues.at(
            format!("{path}.args"),
            IssueCode::InvalidValue,
            format!("at most {VERSION_CHECK_ARGS_MAX} args"),
        );
    }
    let args: Vec<String> = raw
        .args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| check_literal(arg, issues, &format!("{path}.args[{index}]")))
        .collect();
    let pattern = compile(&raw.pattern);
    if let Err(message) = &pattern {
        issues.at(
            format!("{path}.pattern"),
            IssueCode::InvalidValue,
            message.clone(),
        );
    }
    let timeout = match raw.timeout_seconds {
        None => VERSION_CHECK_TIMEOUT_DEFAULT,
        Some(seconds) if (1..=VERSION_CHECK_TIMEOUT_MAX_SECONDS).contains(&seconds) => {
            Duration::from_secs(seconds)
        }
        Some(_) => {
            issues.at(
                format!("{path}.timeoutSeconds"),
                IssueCode::TimeoutOutOfRange,
                format!("timeoutSeconds must be 1 to {VERSION_CHECK_TIMEOUT_MAX_SECONDS}"),
            );
            VERSION_CHECK_TIMEOUT_DEFAULT
        }
    };
    if issues.count() != start {
        return None;
    }
    Some(VersionCheck {
        args,
        pattern: pattern.ok()?,
        timeout,
    })
}

/// Compiles the pattern unanchored, within the settings pattern limits,
/// and requires exactly one capture group.
fn compile(source: &str) -> Result<Regex, String> {
    if source.is_empty() {
        return Err("pattern must not be empty".to_owned());
    }
    if source.chars().count() > PATTERN_MAX {
        return Err(format!("pattern is longer than {PATTERN_MAX} characters"));
    }
    let regex = RegexBuilder::new(source)
        .size_limit(PATTERN_SIZE_LIMIT)
        .build()
        .map_err(|_| "pattern does not compile or exceeds the size limit".to_owned())?;
    if regex.captures_len() != 2 {
        return Err("pattern must have exactly one capture group".to_owned());
    }
    Ok(regex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::issue::Issues;

    fn raw(args: &[&str], pattern: &str, timeout_seconds: Option<u64>) -> RawVersionCheck {
        RawVersionCheck {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            pattern: pattern.to_owned(),
            timeout_seconds,
        }
    }

    fn run(block: &RawVersionCheck) -> (Option<VersionCheck>, Vec<(String, IssueCode)>) {
        let mut issues = Issues::new();
        let checked = check(
            Some(block),
            &mut FileIssues::new("provider.yaml", &mut issues),
        );
        let codes = issues
            .into_vec()
            .into_iter()
            .map(|issue| (issue.location.to_string(), issue.code))
            .collect();
        (checked, codes)
    }

    #[test]
    fn a_valid_block_keeps_its_args_pattern_and_default_timeout() {
        let (checked, codes) = run(&raw(&["--version"], r"^nats-server: v(\S+)$", None));

        assert_eq!(codes, []);
        let checked = checked.unwrap();
        assert_eq!(checked.args(), ["--version"]);
        assert_eq!(checked.pattern(), r"^nats-server: v(\S+)$");
        assert_eq!(checked.timeout(), Duration::from_secs(15));
    }

    #[test]
    fn the_version_comes_from_the_first_matching_line_of_stdout_then_stderr() {
        let postgres = run(&raw(
            &["--version"],
            r"^postgres \(PostgreSQL\) (\S+)$",
            Some(5),
        ))
        .0
        .unwrap();
        let traefik = run(&raw(&["version"], r"^Version:\s+(\S+)$", None))
            .0
            .unwrap();

        assert_eq!(
            postgres.read_version("postgres (PostgreSQL) 18.6\n", ""),
            Some("18.6")
        );
        assert_eq!(
            traefik.read_version(
                "Version:      3.7.15\r\nCodename:     x\r\nGo version:   go1.25\r\n",
                ""
            ),
            Some("3.7.15")
        );
        assert_eq!(
            postgres.read_version("warning: locale\n", "postgres (PostgreSQL) 17.11\n"),
            Some("17.11")
        );
        assert_eq!(postgres.read_version("PostgreSQL 18.6\n", ""), None);
    }

    #[test]
    fn each_broken_rule_gets_its_code() {
        let cases = [
            (
                raw(
                    &["a", "b", "c", "d", "e", "f", "g", "h", "i"],
                    r"v(\S+)",
                    None,
                ),
                "runtime.versionCheck.args",
                IssueCode::InvalidValue,
            ),
            (
                raw(&["${instance.data}"], r"v(\S+)", None),
                "runtime.versionCheck.args[0]",
                IssueCode::TokenNotAllowedHere,
            ),
            (
                raw(&["--version"], r"v\S+", None),
                "runtime.versionCheck.pattern",
                IssueCode::InvalidValue,
            ),
            (
                raw(&["--version"], r"(v)(\S+)", None),
                "runtime.versionCheck.pattern",
                IssueCode::InvalidValue,
            ),
            (
                raw(&["--version"], r"v(\S+", None),
                "runtime.versionCheck.pattern",
                IssueCode::InvalidValue,
            ),
            (
                raw(&["--version"], &format!("({})", "a".repeat(600)), None),
                "runtime.versionCheck.pattern",
                IssueCode::InvalidValue,
            ),
            (
                raw(&["--version"], r"v(\S+)", Some(0)),
                "runtime.versionCheck.timeoutSeconds",
                IssueCode::TimeoutOutOfRange,
            ),
            (
                raw(&["--version"], r"v(\S+)", Some(61)),
                "runtime.versionCheck.timeoutSeconds",
                IssueCode::TimeoutOutOfRange,
            ),
        ];
        for (block, location, code) in cases {
            let (checked, codes) = run(&block);
            assert_eq!(codes, [(location.to_owned(), code)], "{location}");
            assert!(checked.is_none());
        }
    }
}
