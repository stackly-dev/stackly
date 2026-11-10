//! The manifest `remedies` block (spec 0034, *Blok remedies*): repair steps
//! a provider writes for a start failure or a readiness problem, which the
//! doctor shows with the finding it matches.
//!
//! A remedy is only text: steps, commands for you to copy, and paths to look
//! at. Stackly never runs a command or removes a path of a remedy. The block
//! is checked when the package loads: names, lengths, control characters,
//! the `when` conditions, and the tokens of the remedy position, which never
//! take a secret or a setting. A rendered command quotes every token value a
//! shell would split or expand, so it can be pasted into Terminal as is.

use std::collections::BTreeSet;
use std::path::PathBuf;

use stackly_domain::{FailureCode, ReasonCode};

use super::interpolate::{
    Interp, PathExpr, PathPosition, Piece, TypeEnv, ValuePosition, check_literal, check_path,
    check_value,
};
use super::issue::{FileIssues, IssueCode};
use super::manifest::Manifest;
use super::plan::{PlanContext, PlanError, Resolver, Value};
use super::raw::{RawRemedy, RawRemedyStep, RawRemedyWhen};

/// Most entries one `remedies` block holds.
pub const REMEDIES_MAX: usize = 8;
/// Longest `title`, in characters.
pub const REMEDY_TITLE_MAX: usize = 80;
/// Most `steps` of one entry.
pub const REMEDY_STEPS_MAX: usize = 6;
/// Longest step `text`, in characters.
pub const REMEDY_TEXT_MAX: usize = 300;
/// Longest step `command`, in characters, before it is rendered.
pub const REMEDY_COMMAND_MAX: usize = 500;
/// Longest step `path` or `evidence.pidFile`, in characters, before it is
/// rendered.
pub const REMEDY_PATH_MAX: usize = 300;
/// Most codes of `failureCodes` or `readinessReasons`.
pub const REMEDY_WHEN_CODES_MAX: usize = 8;
/// Longest `excerptContains` or `readinessDetailContains`, in characters.
pub const REMEDY_WHEN_TEXT_MAX: usize = 200;
/// Longest `name`, in bytes.
const REMEDY_NAME_MAX: usize = 32;

/// One checked `remedies` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemedySpec {
    pub(crate) name: String,
    pub(crate) when: RemedyWhen,
    pub(crate) pid_file: Option<PathExpr>,
    pub(crate) title: String,
    pub(crate) steps: Vec<RemedyStepSpec>,
}

impl RemedySpec {
    /// Returns the entry name, unique in the manifest.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns whether the entry names a PID file the doctor reads.
    #[must_use]
    pub const fn has_pid_file(&self) -> bool {
        self.pid_file.is_some()
    }

    /// Whether the entry matches `subject`: every condition of `when` that
    /// is set holds. A failure condition never matches a readiness problem,
    /// and a readiness condition never matches a failure.
    #[must_use]
    pub fn matches(&self, subject: &RemedySubject<'_>) -> bool {
        self.when.matches(subject)
    }
}

/// The conditions of an entry; at least one is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemedyWhen {
    pub(crate) failure_codes: Vec<FailureCode>,
    pub(crate) excerpt_contains: Option<String>,
    pub(crate) readiness_reasons: Vec<ReasonCode>,
    pub(crate) readiness_detail_contains: Option<String>,
}

impl RemedyWhen {
    fn matches(&self, subject: &RemedySubject<'_>) -> bool {
        match *subject {
            RemedySubject::Failure { code, excerpt } => {
                self.readiness_reasons.is_empty()
                    && self.readiness_detail_contains.is_none()
                    && (self.failure_codes.is_empty()
                        || code.is_some_and(|code| self.failure_codes.contains(code)))
                    && self
                        .excerpt_contains
                        .as_deref()
                        .is_none_or(|needle| excerpt.iter().any(|line| line.contains(needle)))
            }
            RemedySubject::Readiness { reason, detail } => {
                self.failure_codes.is_empty()
                    && self.excerpt_contains.is_none()
                    && (self.readiness_reasons.is_empty()
                        || self
                            .readiness_reasons
                            .iter()
                            .any(|known| known.as_str() == reason))
                    && self
                        .readiness_detail_contains
                        .as_deref()
                        .is_none_or(|needle| detail.is_some_and(|detail| detail.contains(needle)))
            }
        }
    }
}

/// One checked step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemedyStepSpec {
    pub(crate) text: String,
    pub(crate) command: Option<Interp>,
    pub(crate) path: Option<Interp>,
}

/// What a remedy is matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemedySubject<'a> {
    /// A `failed` instance: the code of its last failure, if the daemon
    /// knows it, and the excerpt of its output (spec 0033).
    Failure {
        /// `lastFailure.code`.
        code: Option<&'a FailureCode>,
        /// The excerpt lines; `excerptContains` matches one of them.
        excerpt: &'a [String],
    },
    /// A `running` instance whose readiness is `not_ready`.
    Readiness {
        /// The readiness reason.
        reason: &'a str,
        /// The readiness detail.
        detail: Option<&'a str>,
    },
}

/// A remedy rendered for one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderedRemedy {
    /// The entry name.
    pub name: String,
    /// The title.
    pub title: String,
    /// The steps, in manifest order.
    pub steps: Vec<RenderedRemedyStep>,
    /// The absolute path of the PID file the entry names.
    pub pid_file: Option<PathBuf>,
}

/// One rendered step.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderedRemedyStep {
    /// What to do.
    pub text: String,
    /// A command for you to copy and run yourself; every token value a
    /// shell would split or expand is single quoted.
    pub command: Option<String>,
    /// A path to look at, as rendered.
    pub path: Option<String>,
}

/// Checks the `remedies` block and returns its entries in manifest order.
/// A package without the block has none.
pub(crate) fn check(
    raw: Option<&[RawRemedy]>,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
) -> Vec<RemedySpec> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    if raw.is_empty() || raw.len() > REMEDIES_MAX {
        issues.at(
            "remedies",
            IssueCode::InvalidValue,
            format!("remedies holds 1 to {REMEDIES_MAX} entries"),
        );
    }
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for (index, entry) in raw.iter().enumerate() {
        let start = issues.count();
        let path = format!("remedies[{index}]");
        if !is_remedy_name(&entry.name) {
            issues.at(
                format!("{path}.name"),
                IssueCode::InvalidName,
                "name must be a lowercase letter, then at most 31 lowercase letters, digits, or _",
            );
        } else if !seen.insert(entry.name.as_str()) {
            issues.at(
                format!("{path}.name"),
                IssueCode::InvalidName,
                format!("the remedy name {} is used twice", entry.name),
            );
        }
        let title = text(
            &entry.title,
            REMEDY_TITLE_MAX,
            &format!("{path}.title"),
            issues,
        );
        let when = when(&entry.when, &format!("{path}.when"), issues);
        let pid_file = entry
            .evidence
            .as_ref()
            .and_then(|evidence| pid_file(&evidence.pid_file, env, issues, &path));
        let steps = steps(&entry.steps, env, issues, &path);
        if let (Some(title), Some(when)) = (title, when)
            && issues.count() == start
        {
            entries.push(RemedySpec {
                name: entry.name.clone(),
                when,
                pid_file,
                title,
                steps,
            });
        }
    }
    entries
}

/// Checks the `when` table.
fn when(raw: &RawRemedyWhen, at: &str, issues: &mut FileIssues<'_>) -> Option<RemedyWhen> {
    if raw.failure_codes.is_none()
        && raw.excerpt_contains.is_none()
        && raw.readiness_reasons.is_none()
        && raw.readiness_detail_contains.is_none()
    {
        issues.at(
            at,
            IssueCode::InvalidValue,
            "when sets at least one of failureCodes, excerptContains, readinessReasons, or readinessDetailContains",
        );
        return None;
    }
    let start = issues.count();
    let failure_codes = codes(
        raw.failure_codes.as_deref(),
        &format!("{at}.failureCodes"),
        issues,
    );
    let readiness_reasons = codes(
        raw.readiness_reasons.as_deref(),
        &format!("{at}.readinessReasons"),
        issues,
    );
    let excerpt_contains = raw
        .excerpt_contains
        .as_deref()
        .and_then(|needle| contains(needle, &format!("{at}.excerptContains"), issues));
    let readiness_detail_contains = raw
        .readiness_detail_contains
        .as_deref()
        .and_then(|needle| contains(needle, &format!("{at}.readinessDetailContains"), issues));
    (issues.count() == start).then_some(RemedyWhen {
        failure_codes,
        excerpt_contains,
        readiness_reasons,
        readiness_detail_contains,
    })
}

/// Checks a list of codes: 1 to 8, each a code of the domain.
fn codes<T: std::str::FromStr>(
    raw: Option<&[String]>,
    at: &str,
    issues: &mut FileIssues<'_>,
) -> Vec<T> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    if raw.is_empty() || raw.len() > REMEDY_WHEN_CODES_MAX {
        issues.at(
            at,
            IssueCode::InvalidValue,
            format!("list 1 to {REMEDY_WHEN_CODES_MAX} codes"),
        );
        return Vec::new();
    }
    let mut parsed = Vec::with_capacity(raw.len());
    for (index, code) in raw.iter().enumerate() {
        match code.parse() {
            Ok(code) => parsed.push(code),
            Err(_) => issues.at(
                format!("{at}[{index}]"),
                IssueCode::InvalidValue,
                "a code is 1 to 64 bytes: a lowercase letter, then lowercase letters, digits, and underscores",
            ),
        }
    }
    parsed
}

/// Checks a text a condition looks for: 1 to 200 characters without
/// control characters, matched as written.
fn contains(needle: &str, at: &str, issues: &mut FileIssues<'_>) -> Option<String> {
    within(needle, REMEDY_WHEN_TEXT_MAX, at, issues).then(|| needle.to_owned())
}

/// Checks the `steps`.
fn steps(
    raw: &[RawRemedyStep],
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Vec<RemedyStepSpec> {
    if raw.is_empty() || raw.len() > REMEDY_STEPS_MAX {
        issues.at(
            format!("{path}.steps"),
            IssueCode::InvalidValue,
            format!("steps holds 1 to {REMEDY_STEPS_MAX} steps"),
        );
    }
    let mut steps = Vec::with_capacity(raw.len());
    for (index, step) in raw.iter().enumerate() {
        let at = format!("{path}.steps[{index}]");
        let text = text(&step.text, REMEDY_TEXT_MAX, &format!("{at}.text"), issues);
        let command = step.command.as_deref().map(|command| {
            value(
                command,
                REMEDY_COMMAND_MAX,
                env,
                issues,
                &format!("{at}.command"),
            )
        });
        let shown = step
            .path
            .as_deref()
            .map(|shown| value(shown, REMEDY_PATH_MAX, env, issues, &format!("{at}.path")));
        if let Some(text) = text
            && command.as_ref().is_none_or(Option::is_some)
            && shown.as_ref().is_none_or(Option::is_some)
        {
            steps.push(RemedyStepSpec {
                text,
                command: command.flatten(),
                path: shown.flatten(),
            });
        }
    }
    steps
}

/// Checks a title or step text: its length, then that it holds no token.
fn text(raw: &str, max: usize, at: &str, issues: &mut FileIssues<'_>) -> Option<String> {
    if !within(raw, max, at, issues) {
        return None;
    }
    check_literal(raw, issues, at)
}

/// Checks a `command` or `path`: its length, then its tokens.
fn value(
    raw: &str,
    max: usize,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    at: &str,
) -> Option<Interp> {
    if !within(raw, max, at, issues) {
        return None;
    }
    check_value(raw, ValuePosition::RemedyText, env, issues, at)
}

/// Checks `evidence.pidFile`: the remedy tokens, then a file under
/// `${instance.data}` or `${instance.config}`.
fn pid_file(
    raw: &str,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Option<PathExpr> {
    let at = format!("{path}.evidence.pidFile");
    value(raw, REMEDY_PATH_MAX, env, issues, &at)?;
    check_path(raw, PathPosition::RemedyPidFile, env, issues, &at)
}

/// Whether `raw` is 1 to `max` characters without a control character;
/// reports `invalid_value` at `at` when it is not.
fn within(raw: &str, max: usize, at: &str, issues: &mut FileIssues<'_>) -> bool {
    let len = raw.chars().count();
    let ok = (1..=max).contains(&len) && !raw.chars().any(char::is_control);
    if !ok {
        issues.at(
            at,
            IssueCode::InvalidValue,
            format!("must be 1 to {max} characters without control characters"),
        );
    }
    ok
}

/// Whether `name` matches `^[a-z][a-z0-9_]{0,31}$`.
fn is_remedy_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    matches!(bytes.first(), Some(b'a'..=b'z'))
        && bytes.len() <= REMEDY_NAME_MAX
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// Renders `remedy` of `manifest` for the instance of `context`, without
/// settings (a remedy cannot name one). Nothing is read from the Keychain
/// or the filesystem.
///
/// # Errors
///
/// Returns the [`PlanError`] of the first token that cannot be rendered for
/// this instance, such as the port of an endpoint that has none yet, or an
/// endpoint of `manifest` that `context` does not hold.
pub fn render_remedy(
    manifest: &Manifest,
    remedy: &RemedySpec,
    context: &PlanContext,
) -> Result<RenderedRemedy, PlanError> {
    let resolver = Resolver::without_managed(manifest, context)?;
    let at = format!("remedies.{}", remedy.name);
    let mut steps = Vec::with_capacity(remedy.steps.len());
    for step in &remedy.steps {
        steps.push(RenderedRemedyStep {
            text: step.text.clone(),
            command: step
                .command
                .as_ref()
                .map(|command| render_command(command, &resolver, &at))
                .transpose()?,
            path: step
                .path
                .as_ref()
                .map(|path| resolver.render(path, &at))
                .transpose()?,
        });
    }
    Ok(RenderedRemedy {
        name: remedy.name.clone(),
        title: remedy.title.clone(),
        steps,
        pid_file: remedy
            .pid_file
            .as_ref()
            .map(|expr| resolver.path(expr))
            .transpose()?,
    })
}

/// Renders a command: the literal text as written, and every token value
/// single quoted when it holds a character outside the set a shell leaves
/// alone, so a folder with a space stays one argument.
fn render_command(
    command: &Interp,
    resolver: &Resolver<'_>,
    at: &str,
) -> Result<String, PlanError> {
    let mut out = String::new();
    for piece in &command.pieces {
        match piece {
            Piece::Literal(text) => out.push_str(text),
            Piece::Token(token) => match resolver.value(token)? {
                Value::Text(text) => out.push_str(&shell_word(&text)),
                Value::Integer(number) => out.push_str(&number.to_string()),
                Value::Boolean(flag) => out.push_str(if flag { "true" } else { "false" }),
                Value::Secret(_) => {
                    return Err(PlanError::UnrepresentableValue {
                        field: format!("{at}: {token}"),
                    });
                }
            },
        }
    }
    Ok(out)
}

/// `text` as one shell word: as is when every character is one a POSIX
/// shell never splits or expands, else in single quotes with each `'`
/// written `'\''`.
fn shell_word(text: &str) -> String {
    let plain = !text.is_empty()
        && text.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '_' | '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-'
                )
        });
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remedy_names_follow_the_pattern() {
        assert!(is_remedy_name("stale_postmaster_pid"));
        assert!(is_remedy_name("r2"));
        assert!(!is_remedy_name("2r"));
        assert!(!is_remedy_name("Stale"));
        assert!(!is_remedy_name("stale-pid"));
        assert!(!is_remedy_name(&"a".repeat(33)));
    }

    #[test]
    fn a_shell_word_is_quoted_only_when_a_shell_would_touch_it() {
        assert_eq!(shell_word("/private/tmp/skt/run"), "/private/tmp/skt/run");
        assert_eq!(
            shell_word("/Users/me/Library/Application Support/Stackly"),
            "'/Users/me/Library/Application Support/Stackly'"
        );
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word("$(id)"), "'$(id)'");
        assert_eq!(shell_word("~/x"), "'~/x'");
        assert_eq!(shell_word(""), "''");
    }

    fn when(
        codes: &[&str],
        excerpt: Option<&str>,
        reasons: &[&str],
        detail: Option<&str>,
    ) -> RemedyWhen {
        RemedyWhen {
            failure_codes: codes.iter().map(|code| code.parse().unwrap()).collect(),
            excerpt_contains: excerpt.map(str::to_owned),
            readiness_reasons: reasons
                .iter()
                .map(|reason| reason.parse().unwrap())
                .collect(),
            readiness_detail_contains: detail.map(str::to_owned),
        }
    }

    #[test]
    fn a_failure_matches_when_its_code_and_one_excerpt_line_match() {
        let rule = when(
            &["crash_loop", "process_exited"],
            Some("postmaster.pid"),
            &[],
            None,
        );
        let exited = FailureCode::PROCESS_EXITED;
        let lines = vec![
            "FATAL:  could not start".to_owned(),
            "FATAL:  lock file \"postmaster.pid\" already exists".to_owned(),
        ];

        assert!(rule.matches(&RemedySubject::Failure {
            code: Some(&exited),
            excerpt: &lines,
        }));
        assert!(!rule.matches(&RemedySubject::Failure {
            code: Some(&FailureCode::SPAWN_FAILED),
            excerpt: &lines,
        }));
        assert!(!rule.matches(&RemedySubject::Failure {
            code: None,
            excerpt: &lines,
        }));
        assert!(!rule.matches(&RemedySubject::Failure {
            code: Some(&exited),
            excerpt: &lines[..1],
        }));
    }

    #[test]
    fn the_excerpt_is_matched_by_case() {
        let rule = when(&[], Some("postmaster.pid"), &[], None);
        let lines = vec!["POSTMASTER.PID".to_owned()];

        assert!(!rule.matches(&RemedySubject::Failure {
            code: None,
            excerpt: &lines,
        }));
    }

    #[test]
    fn failure_and_readiness_conditions_never_cross() {
        let failure = when(&["process_exited"], None, &[], None);
        let readiness = when(&[], None, &[], Some("role \"stackly_readiness\""));
        let detail = "FATAL:  role \"stackly_readiness\" does not exist";

        assert!(!failure.matches(&RemedySubject::Readiness {
            reason: "probe_failed",
            detail: Some(detail),
        }));
        assert!(readiness.matches(&RemedySubject::Readiness {
            reason: "probe_failed",
            detail: Some(detail),
        }));
        assert!(!readiness.matches(&RemedySubject::Readiness {
            reason: "probe_failed",
            detail: None,
        }));
        assert!(!readiness.matches(&RemedySubject::Failure {
            code: Some(&FailureCode::PROCESS_EXITED),
            excerpt: &[detail.to_owned()],
        }));
    }

    #[test]
    fn a_readiness_reason_must_be_listed() {
        let rule = when(&[], None, &["probe_failed"], None);

        assert!(rule.matches(&RemedySubject::Readiness {
            reason: "probe_failed",
            detail: None,
        }));
        assert!(!rule.matches(&RemedySubject::Readiness {
            reason: "probe_timeout",
            detail: None,
        }));
    }
}
