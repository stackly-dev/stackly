//! Checks (b), (g), and (h) of `cargo xtask docs check` (spec 0038):
//! placeholders, the machine enforced part of `.markdownlint.jsonc`, and the
//! approval statement of spec 0037 on `docs/providers/trying-locally.md`.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::Problem;
use super::markdown::Markdown;

/// The whole words that mark unfinished text.
const PLACEHOLDER_WORDS: [&str; 3] = ["TODO", "TBD", "FIXME"];
/// The texts that mark unfinished text anywhere in a word.
const PLACEHOLDER_TEXTS: [&str; 2] = ["[INSERT", "example.com"];

/// The page that quotes the approval statement.
const TRYING_LOCALLY: &str = "docs/providers/trying-locally.md";
/// The fixture that holds the approval statement (spec 0037).
const APPROVAL_STATEMENT: &str = "protocols/fixtures/provider/approval-statement.txt";

/// The placeholder problems of `doc` (check (b)).
#[must_use]
pub fn placeholders(rel: &str, doc: &Markdown) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (line, prose) in doc.prose() {
        for found in placeholders_in(prose) {
            problems.push(Problem::new(rel, line, format!("placeholder `{found}`")));
        }
    }
    problems
}

/// The `[INSERT` problems of `doc`, the only placeholder check that runs on
/// `CODE_OF_CONDUCT.md`.
#[must_use]
pub fn insert_markers(rel: &str, doc: &Markdown) -> Vec<Problem> {
    doc.lines
        .iter()
        .filter(|line| line.raw.contains("[INSERT"))
        .map(|line| Problem::new(rel, line.number, "placeholder `[INSERT`"))
        .collect()
}

/// The placeholders on one prose text, in order of the lists.
#[must_use]
pub fn placeholders_in(prose: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for word in PLACEHOLDER_WORDS {
        if has_word(prose, word) {
            found.push(word);
        }
    }
    for text in PLACEHOLDER_TEXTS {
        if prose.contains(text) {
            found.push(text);
        }
    }
    found
}

/// Whether `word` appears in `text` as a whole word.
fn has_word(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// The lint problems of `doc` (check (g)): inline HTML other than `<a>` and
/// comments, and fenced blocks without a language.
#[must_use]
pub fn lint(rel: &str, doc: &Markdown) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (line, prose) in doc.prose() {
        for tag in html_tags(prose) {
            problems.push(Problem::new(
                rel,
                line,
                format!("inline HTML `<{tag}>` is not allowed; only `<a>` is"),
            ));
        }
    }
    for fence in &doc.fences {
        if fence.lang.is_empty() {
            problems.push(Problem::new(
                rel,
                fence.line,
                "fenced code block has no language",
            ));
        }
    }
    problems
}

/// The names of the HTML tags on one prose line other than `a`.
fn html_tags(prose: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for (start, _) in prose.match_indices('<') {
        let rest = &prose[start + 1..];
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        let next = rest[name.len()..].chars().next();
        let ends_tag = next.is_none_or(|c| c.is_whitespace() || c == '>' || c == '/');
        if ends_tag && !name.eq_ignore_ascii_case("a") {
            tags.push(name);
        }
    }
    tags
}

/// Check (h): when `docs/providers/trying-locally.md` exists, it quotes the
/// approval statement of spec 0037 whole.
///
/// # Errors
///
/// Returns an error when the page exists but cannot be read.
pub fn approval_statement(root: &Path) -> Result<Vec<Problem>> {
    let page = root.join(TRYING_LOCALLY);
    if !page.is_file() {
        return Ok(Vec::new());
    }
    let Ok(statement) = fs::read_to_string(root.join(APPROVAL_STATEMENT)) else {
        return Ok(vec![Problem::new(
            TRYING_LOCALLY,
            1,
            format!(
                "`{APPROVAL_STATEMENT}` is missing, so the approval statement cannot be checked"
            ),
        )]);
    };
    let text =
        fs::read_to_string(&page).with_context(|| format!("cannot read {}", page.display()))?;
    let unquoted: String = text
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            trimmed.trim_start_matches('>').trim_start()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if normalize(&unquoted).contains(&normalize(&statement)) {
        Ok(Vec::new())
    } else {
        Ok(vec![Problem::new(
            TRYING_LOCALLY,
            1,
            format!("the approval statement of `{APPROVAL_STATEMENT}` is not quoted word for word"),
        )])
    }
}

/// `text` with every run of white space turned into one space.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::super::fixture;
    use super::{html_tags, placeholders_in};

    // covers: AC-9 (spec 0038)
    #[test]
    fn placeholders_are_whole_words_or_marker_texts() {
        assert_eq!(placeholders_in("a TODO b"), vec!["TODO"]);
        assert!(placeholders_in("TODOS and todo and TBDX").is_empty());
        assert_eq!(
            placeholders_in("[INSERT CONTACT] at example.com"),
            vec!["[INSERT", "example.com"]
        );
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn a_placeholder_outside_code_is_one_problem() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "SECURITY.md",
            "# S\n\nThe target is TBD.\n\n`TBD` in code is fine.\n\n```text\nTODO\n```\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec!["SECURITY.md:3: placeholder `TBD`"]
        );
    }

    // covers: AC-9, AC-14 (spec 0038)
    #[test]
    fn tags_other_than_a_and_comments_are_found() {
        assert_eq!(
            html_tags("a <br> b <a href=\"x\">c</a> <div class=x>"),
            vec!["br", "div"]
        );
        assert!(html_tags("a < b and 1<2").is_empty());
    }

    // covers: AC-9, AC-14 (spec 0038)
    #[test]
    fn a_br_tag_is_one_problem_but_code_spans_and_autolinks_are_not() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "SECURITY.md",
            "# S\n\nline<br>\n\n`<data>` and `<runId>` <https://github.com> <!-- note -->\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec!["SECURITY.md:3: inline HTML `<br>` is not allowed; only `<a>` is"]
        );
    }

    // covers: AC-9, AC-14 (spec 0038)
    #[test]
    fn a_fence_without_a_language_is_one_problem() {
        let repo = fixture::repo();
        fixture::write(repo.path(), "SECURITY.md", "# S\n\n```\nplain\n```\n");
        assert_eq!(
            fixture::lines(repo.path()),
            vec!["SECURITY.md:3: fenced code block has no language"]
        );
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn the_code_of_conduct_gets_only_links_and_the_insert_marker() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "CODE_OF_CONDUCT.md",
            "# Code\n\nTODO <br>\n\n```\nx\n```\n\nContact [INSERT CONTACT METHOD].\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec!["CODE_OF_CONDUCT.md:9: placeholder `[INSERT`"]
        );
    }

    // covers: AC-9, AC-12 (spec 0038)
    #[test]
    fn the_approval_statement_must_be_quoted_word_for_word() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "protocols/fixtures/provider/approval-statement.txt",
            "This package can run\nthe executables it names.\n",
        );
        fixture::write(
            repo.path(),
            "docs/providers/trying-locally.md",
            "# Trying\n\n> This package can run\n> the executables it names.\n",
        );
        let lines = fixture::lines(repo.path());
        assert!(
            !lines.iter().any(|line| line.contains("approval statement")),
            "{lines:?}"
        );
        fixture::write(
            repo.path(),
            "docs/providers/trying-locally.md",
            "# Trying\n\n> This package may run\n> the executables it names.\n",
        );
        let lines = fixture::lines(repo.path());
        assert!(
            lines.contains(&"docs/providers/trying-locally.md:1: the approval statement of `protocols/fixtures/provider/approval-statement.txt` is not quoted word for word".to_owned()),
            "{lines:?}"
        );
    }

    // covers: AC-9, AC-12 (spec 0038)
    #[test]
    fn a_statement_one_word_off_is_exactly_one_problem() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "protocols/fixtures/provider/approval-statement.txt",
            "This package can run the executables it names.\n",
        );
        fixture::write(
            repo.path(),
            "docs/providers/trying-locally.md",
            "# Trying\n\n> This package can run the programs it names.\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec![
                "docs/providers/trying-locally.md:1: the approval statement of `protocols/fixtures/provider/approval-statement.txt` is not quoted word for word"
            ]
        );
    }

    // covers: AC-9, AC-12 (spec 0038)
    #[test]
    fn a_trying_locally_page_without_the_statement_fixture_is_one_problem() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "docs/providers/trying-locally.md",
            "# Trying\n\n> This package can run the executables it names.\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec![
                "docs/providers/trying-locally.md:1: `protocols/fixtures/provider/approval-statement.txt` is missing, so the approval statement cannot be checked"
            ]
        );
    }

    // covers: AC-9, AC-12 (spec 0038)
    #[test]
    fn no_trying_locally_page_needs_no_statement_fixture() {
        let repo = fixture::repo();
        assert!(fixture::lines(repo.path()).is_empty());
    }
}
