//! Check (c) of `cargo xtask docs check` (spec 0038): every `cargo xtask` and
//! `stackly` command in a `bash` block names a task or a command that exists.
//! The truth is two static tables, the xtask `USAGE` text and the `stackly`
//! man page list; a command found in a doc is never run.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::Problem;
use super::markdown::Markdown;

/// The task paths of xtask and the page paths of `stackly`.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    /// Every xtask task path, such as `provider check`.
    xtask: BTreeSet<Vec<String>>,
    /// Every `stackly` page path, such as `stackly-daemon-status`.
    stackly: BTreeSet<String>,
}

impl Tables {
    /// The tables of the repo at `root`: the `USAGE` text of this binary and
    /// the pages `stackly manpage --json` lists.
    ///
    /// # Errors
    ///
    /// Returns an error when `stackly-cli` cannot be built or run, or its
    /// output cannot be read.
    pub fn load(root: &Path) -> Result<Self> {
        let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
            .current_dir(root)
            .args([
                "run",
                "-q",
                "--locked",
                "-p",
                "stackly-cli",
                "--",
                "manpage",
                "--json",
            ])
            .output()
            .context("cannot run `cargo run -p stackly-cli -- manpage --json`")?;
        if !output.status.success() {
            bail!(
                "cannot read the stackly command table: `stackly manpage --json` exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("cannot read the output of `stackly manpage --json`")?;
        let pages = value
            .pointer("/result/pages")
            .and_then(serde_json::Value::as_array)
            .context("`stackly manpage --json` has no result.pages")?;
        let files: Vec<&str> = pages
            .iter()
            .filter_map(|page| page.get("file").and_then(serde_json::Value::as_str))
            .collect();
        if files.is_empty() {
            bail!("`stackly manpage --json` lists no pages");
        }
        Ok(Self::from_parts(crate::USAGE, &files))
    }

    /// Tables from a `USAGE` text and a list of man page file names (with or
    /// without the `.1` suffix).
    #[must_use]
    pub fn from_parts(usage: &str, pages: &[&str]) -> Self {
        Self {
            xtask: usage_paths(usage),
            stackly: pages
                .iter()
                .map(|page| page.strip_suffix(".1").unwrap_or(page).to_owned())
                .collect(),
        }
    }

    /// The command problems of the `bash` blocks of `doc`.
    #[must_use]
    pub fn check(&self, rel: &str, doc: &Markdown) -> Vec<Problem> {
        let mut problems = Vec::new();
        for fence in doc.fences.iter().filter(|fence| fence.lang == "bash") {
            for (line, command) in commands(&fence.body) {
                for piece in split_pieces(&command) {
                    if let Some(message) = self.problem(&tokens(&piece)) {
                        problems.push(Problem::new(rel, line, message));
                    }
                }
            }
        }
        problems
    }

    /// The problem with one simple command, if it is an unknown xtask task or
    /// `stackly` command.
    fn problem(&self, tokens: &[String]) -> Option<String> {
        let start = tokens
            .iter()
            .position(|token| !is_assignment(token))
            .unwrap_or(tokens.len());
        let tokens = &tokens[start..];
        let first = tokens.first().map(String::as_str);
        let second = tokens.get(1).map(String::as_str);
        match (first, second) {
            (Some("cargo"), Some("xtask")) => self.xtask_problem(&candidates(&tokens[2..])),
            (Some("stackly"), _) => self.stackly_problem(&candidates(&tokens[1..])),
            (Some("cargo"), Some("run")) => {
                let cli = tokens.windows(2).any(|pair| {
                    matches!(pair[0].as_str(), "-p" | "--package") && pair[1] == "stackly-cli"
                }) || tokens.iter().any(|token| token == "--package=stackly-cli");
                if !cli {
                    return None;
                }
                let rest = tokens
                    .iter()
                    .position(|token| token == "--")
                    .map_or(&[][..], |index| &tokens[index + 1..]);
                self.stackly_problem(&candidates(rest))
            }
            _ => None,
        }
    }

    /// The problem with `cargo xtask <words>`.
    fn xtask_problem(&self, words: &[String]) -> Option<String> {
        let known = (1..=words.len())
            .rev()
            .any(|len| self.xtask.contains(&words[..len]));
        if known {
            None
        } else {
            Some(format!(
                "unknown xtask task `cargo xtask {}`",
                words.join(" ")
            ))
        }
    }

    /// The problem with `stackly <words>`.
    fn stackly_problem(&self, words: &[String]) -> Option<String> {
        for len in (0..=words.len()).rev() {
            let path = std::iter::once("stackly")
                .chain(words[..len].iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join("-");
            if !self.stackly.contains(&path) {
                continue;
            }
            let prefix = format!("{path}-");
            let has_children = self.stackly.iter().any(|page| page.starts_with(&prefix));
            if has_children && len < words.len() {
                break;
            }
            return None;
        }
        Some(format!(
            "unknown stackly command `stackly {}`",
            words.join(" ")
        ))
    }
}

/// The task paths in a `USAGE` text: each line that starts with exactly two
/// spaces and a lowercase letter gives the lowercase words before the first
/// token that starts with `-`, `<`, or `[` (or a gap of two spaces).
fn usage_paths(usage: &str) -> BTreeSet<Vec<String>> {
    let mut paths = BTreeSet::new();
    for line in usage.lines() {
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        if !rest.starts_with(|c: char| c.is_ascii_lowercase()) {
            continue;
        }
        let head = rest.split("  ").next().unwrap_or_default();
        let path: Vec<String> = head
            .split_whitespace()
            .take_while(|word| {
                !word.starts_with(['-', '<', '['])
                    && word.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            })
            .map(str::to_owned)
            .collect();
        if !path.is_empty() {
            paths.insert(path);
        }
    }
    paths
}

/// The commands of a `bash` block body: lines ending in `\` joined with the
/// next, and comments from ` #` (or a leading `#`) to the end dropped. Each
/// command keeps the number of its first line.
fn commands(body: &[(usize, String)]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (number, raw) in body {
        let text = strip_comment(raw);
        let (start, mut joined) = pending.take().unwrap_or((*number, String::new()));
        let trimmed = text.trim_end();
        if let Some(head) = trimmed.strip_suffix('\\') {
            joined.push_str(head);
            joined.push(' ');
            pending = Some((start, joined));
        } else {
            joined.push_str(trimmed);
            if !joined.trim().is_empty() {
                out.push((start, joined));
            }
        }
    }
    if let Some((start, joined)) = pending
        && !joined.trim().is_empty()
    {
        out.push((start, joined));
    }
    out
}

/// `line` without its trailing comment.
fn strip_comment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    match line.find(" #") {
        Some(index) => &line[..index],
        None => line,
    }
}

/// The simple commands of one command line, split at `&&`, `;`, and `|`
/// outside quotes and `$(...)`.
fn split_pieces(command: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '$' if chars.get(i + 1) == Some(&'(') => {
                    depth += 1;
                    current.push_str("$(");
                    i += 2;
                    continue;
                }
                '(' if depth > 0 => depth += 1,
                ')' if depth > 0 => depth -= 1,
                '&' if depth == 0 && chars.get(i + 1) == Some(&'&') => {
                    pieces.push(std::mem::take(&mut current));
                    i += 2;
                    continue;
                }
                ';' | '|' if depth == 0 => {
                    pieces.push(std::mem::take(&mut current));
                    i += 1;
                    continue;
                }
                _ => {}
            },
        }
        current.push(c);
        i += 1;
    }
    pieces.push(current);
    pieces
        .into_iter()
        .map(|piece| piece.trim().to_owned())
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// The words of one simple command, split at white space outside quotes and
/// `$(...)`, with the quotes removed.
fn tokens(piece: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut started = false;
    let chars: Vec<char> = piece.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(open) if c == open => {
                quote = None;
                i += 1;
                continue;
            }
            Some(_) => {}
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    started = true;
                    i += 1;
                    continue;
                }
                '$' if chars.get(i + 1) == Some(&'(') => {
                    depth += 1;
                    current.push_str("$(");
                    started = true;
                    i += 2;
                    continue;
                }
                '(' if depth > 0 => depth += 1,
                ')' if depth > 0 => depth -= 1,
                c if c.is_whitespace() && depth == 0 => {
                    if started {
                        out.push(std::mem::take(&mut current));
                        started = false;
                    }
                    i += 1;
                    continue;
                }
                _ => {}
            },
        }
        current.push(c);
        started = true;
        i += 1;
    }
    if started {
        out.push(current);
    }
    out
}

/// Whether `token` is a `NAME=value` assignment.
fn is_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The candidate words: the words before the first token that starts with
/// `-`, `<`, `[`, or `$`, or that holds `/` or `.`.
fn candidates(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .take_while(|token| !token.starts_with(['-', '<', '[', '$']) && !token.contains(['/', '.']))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::fixture;
    use super::*;

    fn tables() -> Tables {
        fixture::tables()
    }

    fn problems(block: &str) -> Vec<String> {
        let doc = Markdown::parse(&format!("```bash\n{block}\n```\n"));
        tables()
            .check("X.md", &doc)
            .into_iter()
            .map(|problem| problem.message)
            .collect()
    }

    #[test]
    fn usage_lines_give_task_paths() {
        let paths = usage_paths(crate::USAGE);
        for path in [
            vec!["codegen"],
            vec!["check-deps"],
            vec!["provider", "check"],
            vec!["runtime", "spike"],
            vec!["evidence", "new"],
            vec!["docs", "check"],
        ] {
            let path: Vec<String> = path.into_iter().map(str::to_owned).collect();
            assert!(paths.contains(&path), "{path:?} in {paths:?}");
        }
        assert!(
            !paths
                .iter()
                .any(|path| path.iter().any(|word| word.starts_with("--")))
        );
    }

    #[test]
    fn known_commands_pass_even_with_comments_assignments_and_cargo_run() {
        let block = "cargo xtask codegen --check   # fails when files drift\n\
                     STACKLY_ROOT=$(mktemp -d) cargo run -q -p stacklyd -- --print-roots\n\
                     cargo run -q -p stackly-cli -- daemon status --json\n\
                     stackly --version && stackly provider list | head\n\
                     cargo xtask provider check providers/nats \\\n  providers/traefik\n\
                     git config core.hooksPath .githooks";
        assert_eq!(problems(block), Vec::<String>::new());
    }

    #[test]
    fn an_unknown_xtask_task_is_one_problem() {
        assert_eq!(
            problems("cargo xtask nope"),
            vec!["unknown xtask task `cargo xtask nope`"]
        );
        assert_eq!(
            problems("cargo xtask provider nope <dir>"),
            vec!["unknown xtask task `cargo xtask provider nope`"]
        );
    }

    #[test]
    fn an_unknown_stackly_command_is_one_problem() {
        assert_eq!(
            problems("stackly provider nope"),
            vec!["unknown stackly command `stackly provider nope`"]
        );
        assert_eq!(
            problems("cargo run -q -p stackly-cli -- nope"),
            vec!["unknown stackly command `stackly nope`"]
        );
        assert_eq!(
            problems("stackly daemon status extra"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn only_bash_blocks_are_read() {
        let doc = Markdown::parse("```text\ncargo xtask nope\n```\n");
        assert!(tables().check("X.md", &doc).is_empty());
    }

    #[test]
    fn a_problem_names_the_line_of_the_command() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "SECURITY.md",
            "# S\n\n```bash\ncargo xtask fuzz list\nstackly provider nope\n```\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec![
                "SECURITY.md:4: unknown xtask task `cargo xtask fuzz list`",
                "SECURITY.md:5: unknown stackly command `stackly provider nope`",
            ]
        );
    }

    #[test]
    fn pieces_split_outside_quotes_and_substitutions() {
        assert_eq!(
            split_pieces("A=$(echo a; b) x && y | z; 'q;r'"),
            vec!["A=$(echo a; b) x", "y", "z", "'q;r'"]
        );
        assert_eq!(
            tokens("A=$(mktemp -d) cargo \"x y\""),
            vec!["A=$(mktemp -d)", "cargo", "x y"]
        );
    }
}
