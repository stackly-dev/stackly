//! Check (a) of `cargo xtask docs check` (spec 0038): every relative link and
//! anchor points at a file or folder and a heading that exist.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::Problem;
use super::markdown::Markdown;

/// Checks links, keeping the heading slugs of each target file it read.
#[derive(Debug)]
pub struct Checker {
    /// The repo root, for links that start with `/`.
    root: PathBuf,
    /// The slugs of each Markdown file read so far.
    slugs: HashMap<PathBuf, Option<Vec<String>>>,
}

impl Checker {
    /// A checker for the repo at `root`.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            slugs: HashMap::new(),
        }
    }

    /// The link problems of `doc`, the file `rel` at `abs`.
    pub fn check(&mut self, rel: &str, abs: &Path, doc: &Markdown) -> Vec<Problem> {
        let own = doc.slugs();
        let mut problems = Vec::new();
        for (line, prose) in doc.prose() {
            for dest in destinations(prose) {
                if let Some(message) = self.problem(abs, &own, &dest) {
                    problems.push(Problem::new(rel, line, message));
                }
            }
        }
        problems
    }

    /// The problem with one link destination `dest` written in `abs`, whose
    /// own slugs are `own`.
    pub fn problem(&mut self, abs: &Path, own: &[String], dest: &str) -> Option<String> {
        let dest = dest.trim();
        if dest.is_empty() || dest.contains("://") || dest.starts_with("mailto:") {
            return None;
        }
        let (path_part, anchor) = match dest.split_once('#') {
            Some((path, anchor)) => (path, Some(anchor)),
            None => (dest, None),
        };
        let path_part = path_part.split('?').next().unwrap_or_default();
        let path_part = path_part.replace("%20", " ");
        let target = if path_part.is_empty() {
            abs.to_path_buf()
        } else if let Some(from_root) = path_part.strip_prefix('/') {
            normalize(&self.root.join(from_root))
        } else {
            let base = abs.parent().unwrap_or(&self.root);
            normalize(&base.join(&path_part))
        };
        if !target.exists() {
            return Some(format!("link target `{dest}` does not exist"));
        }
        let anchor = anchor?;
        if anchor.is_empty() || target.extension().is_none_or(|ext| ext != "md") {
            return None;
        }
        let found = if target == abs {
            own.iter().any(|slug| slug == anchor)
        } else {
            self.slugs_of(&target)
                .is_some_and(|slugs| slugs.iter().any(|slug| slug == anchor))
        };
        if found {
            None
        } else {
            Some(format!(
                "anchor `#{anchor}` of link `{dest}` matches no heading"
            ))
        }
    }

    /// The slugs of the Markdown file at `path`, read once.
    fn slugs_of(&mut self, path: &Path) -> Option<&Vec<String>> {
        self.slugs
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                fs::read_to_string(path)
                    .ok()
                    .map(|text| Markdown::parse(&text).slugs())
            })
            .as_ref()
    }
}

/// `path` with `.` and `..` resolved by text.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Every link destination on one prose line: inline links and images
/// `[text](dest)`, and reference definitions `[id]: dest`.
#[must_use]
pub fn destinations(prose: &str) -> Vec<String> {
    let mut found = Vec::new();
    let trimmed = prose.trim_start();
    if trimmed.starts_with('[')
        && let Some(close) = trimmed.find("]:")
        && !trimmed[1..close].contains(']')
    {
        let rest = trimmed[close + 2..].trim();
        let dest = rest
            .strip_prefix('<')
            .and_then(|inner| inner.split_once('>').map(|(dest, _)| dest))
            .unwrap_or_else(|| rest.split_whitespace().next().unwrap_or_default());
        if !dest.is_empty() {
            found.push(dest.to_owned());
        }
        return found;
    }
    let mut rest = prose;
    while let Some(index) = rest.find("](") {
        let after = &rest[index + 2..];
        let (dest, used) = inline_destination(after);
        if !dest.is_empty() {
            found.push(dest);
        }
        rest = &after[used..];
    }
    found
}

/// The destination at the start of `after` (the text after `](`), and how
/// many bytes it used.
fn inline_destination(after: &str) -> (String, usize) {
    let leading = after.len() - after.trim_start().len();
    let body = &after[leading..];
    if let Some(inner) = body.strip_prefix('<') {
        return match inner.find('>') {
            Some(end) => (inner[..end].to_owned(), leading + end + 2),
            None => (String::new(), leading),
        };
    }
    let mut depth = 0usize;
    for (index, c) in body.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return (body[..index].to_owned(), leading + index + 1),
            ')' => depth -= 1,
            c if c.is_whitespace() => return (body[..index].to_owned(), leading + index),
            _ => {}
        }
    }
    (body.to_owned(), after.len())
}

#[cfg(test)]
mod tests {
    use super::super::fixture;
    use super::destinations;

    // covers: AC-9 (spec 0038)
    #[test]
    fn inline_links_images_and_reference_definitions_are_found() {
        assert_eq!(
            destinations("see [a](b.md#x \"t\") and ![i](img.png) and [c](<d e.md>)"),
            vec!["b.md#x", "img.png", "d e.md"]
        );
        assert_eq!(destinations("[id]: ../x.md"), vec!["../x.md"]);
        assert_eq!(
            destinations("[a](https://x.dev/(y))"),
            vec!["https://x.dev/(y)"]
        );
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn a_link_to_a_missing_file_is_one_problem() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "SECURITY.md",
            "# S\n\nRead [this](docs/missing.md).\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec!["SECURITY.md:3: link target `docs/missing.md` does not exist"]
        );
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn a_wrong_anchor_is_one_problem_and_a_right_one_none() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "SECURITY.md",
            "# Security\n\n## What to expect\n\n[a](#what-to-expect) [b](CONTRIBUTING.md#set-up) [c](#nope)\n",
        );
        assert_eq!(
            fixture::lines(repo.path()),
            vec!["SECURITY.md:5: anchor `#nope` of link `#nope` matches no heading"]
        );
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn external_links_folders_and_links_in_code_are_not_checked() {
        let repo = fixture::repo();
        fixture::write(
            repo.path(),
            "SECURITY.md",
            "# S\n\n[a](https://github.com/x) [b](mailto:a@b.dev) [c](providers/) `[d](nope.md)`\n\n```text\n[e](nope.md)\n```\n",
        );
        assert_eq!(fixture::lines(repo.path()), Vec::<String>::new());
    }
}
