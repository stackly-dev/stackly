//! A small Markdown reader for `cargo xtask docs check` (spec 0038): fenced
//! code blocks, code spans, autolinks, HTML comments, and ATX headings with
//! their GitHub slugs. It reads only what the checks need and never renders.

use std::collections::HashMap;

/// One line of a Markdown file.
#[derive(Debug, Clone)]
pub struct Line {
    /// The 1 based line number.
    pub number: usize,
    /// The line as written.
    pub raw: String,
    /// The line outside fenced blocks, with code spans, autolinks, and HTML
    /// comments blanked out; `None` inside a fenced block or on its fences.
    pub prose: Option<String>,
}

/// One fenced code block.
#[derive(Debug, Clone)]
pub struct Fence {
    /// The line of the opening fence.
    pub line: usize,
    /// The first word of the info string, empty when it has none.
    pub lang: String,
    /// The lines between the fences, with their numbers.
    pub body: Vec<(usize, String)>,
}

/// One ATX heading.
#[derive(Debug, Clone)]
pub struct Heading {
    /// The line it sits on.
    pub line: usize,
    /// How many `#` it starts with.
    pub level: usize,
    /// The heading text without the `#` markers.
    pub text: String,
}

/// A read Markdown file.
#[derive(Debug, Clone, Default)]
pub struct Markdown {
    /// Every line.
    pub lines: Vec<Line>,
    /// Every fenced block, in order.
    pub fences: Vec<Fence>,
    /// Every heading outside fenced blocks, in order.
    pub headings: Vec<Heading>,
}

impl Markdown {
    /// Reads `text`.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut doc = Self::default();
        let mut open: Option<(usize, Fence)> = None;
        let mut in_comment = false;
        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            let trimmed = raw.trim_start();
            let ticks = trimmed.chars().take_while(|c| *c == '`').count();
            if let Some((width, fence)) = open.as_mut() {
                if ticks >= *width && trimmed[ticks..].trim().is_empty() {
                    let (_, fence) = open.take().expect("a fence is open in this branch");
                    doc.fences.push(fence);
                } else {
                    fence.body.push((number, raw.to_owned()));
                }
                doc.lines.push(Line {
                    number,
                    raw: raw.to_owned(),
                    prose: None,
                });
                continue;
            }
            if ticks >= 3 && !trimmed[ticks..].contains('`') {
                let lang = trimmed[ticks..]
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                open = Some((
                    ticks,
                    Fence {
                        line: number,
                        lang,
                        body: Vec::new(),
                    },
                ));
                doc.lines.push(Line {
                    number,
                    raw: raw.to_owned(),
                    prose: None,
                });
                continue;
            }
            let prose = mask(raw, &mut in_comment);
            if let Some(heading) = heading(raw, number) {
                doc.headings.push(heading);
            }
            doc.lines.push(Line {
                number,
                raw: raw.to_owned(),
                prose: Some(prose),
            });
        }
        if let Some((_, fence)) = open {
            doc.fences.push(fence);
        }
        doc
    }

    /// The prose lines, with their numbers.
    pub fn prose(&self) -> impl Iterator<Item = (usize, &str)> {
        self.lines
            .iter()
            .filter_map(|line| line.prose.as_deref().map(|prose| (line.number, prose)))
    }

    /// The GitHub anchor slugs of every heading.
    #[must_use]
    pub fn slugs(&self) -> Vec<String> {
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut slugs = Vec::new();
        for heading in &self.headings {
            let base = slug(&heading.text);
            let count = seen.entry(base.clone()).or_insert(0);
            if *count == 0 {
                slugs.push(base);
            } else {
                slugs.push(format!("{base}-{count}"));
            }
            *count += 1;
        }
        slugs
    }
}

/// The ATX heading on `raw`, if it is one.
fn heading(raw: &str, number: usize) -> Option<Heading> {
    let indent = raw.len() - raw.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &raw[indent..];
    let level = rest.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let after = &rest[level..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let text = after.trim().trim_end_matches('#').trim_end().to_owned();
    Some(Heading {
        line: number,
        level,
        text,
    })
}

/// The GitHub slug of a heading: lowercase, every character other than a
/// letter, a digit, a space, `-`, or `_` dropped, spaces turned into `-`.
#[must_use]
pub fn slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The contents of every code span on `raw`, in order.
#[must_use]
pub fn code_spans(raw: &str) -> Vec<String> {
    spans(raw)
        .into_iter()
        .map(|(start, end, width)| raw[start + width..end - width].trim().to_owned())
        .collect()
}

/// The byte ranges of the code spans on `raw`, with their backtick width.
fn spans(raw: &str) -> Vec<(usize, usize, usize)> {
    let bytes = raw.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let width = bytes[i..].iter().take_while(|b| **b == b'`').count();
        let mut j = i + width;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let run = bytes[j..].iter().take_while(|b| **b == b'`').count();
                if run == width {
                    closed = Some(j + run);
                    break;
                }
                j += run;
            } else {
                j += 1;
            }
        }
        match closed {
            Some(end) => {
                found.push((i, end, width));
                i = end;
            }
            None => i += width,
        }
    }
    found
}

/// `raw` with code spans, autolinks, and HTML comments replaced by spaces,
/// so offsets stay the same. `in_comment` carries an open comment from the
/// line before.
fn mask(raw: &str, in_comment: &mut bool) -> String {
    let mut out: Vec<u8> = raw.as_bytes().to_vec();
    let blank = |out: &mut Vec<u8>, start: usize, end: usize| {
        for byte in &mut out[start..end] {
            if byte.is_ascii() {
                *byte = b' ';
            }
        }
    };
    for (start, end, _) in spans(raw) {
        blank(&mut out, start, end);
    }
    let mut i = 0;
    if *in_comment {
        match raw.find("-->") {
            Some(end) => {
                blank(&mut out, 0, end + 3);
                *in_comment = false;
                i = end + 3;
            }
            None => {
                blank(&mut out, 0, raw.len());
                return String::from_utf8_lossy(&out).into_owned();
            }
        }
    }
    let masked_spans = String::from_utf8_lossy(&out).into_owned();
    while let Some(offset) = masked_spans[i..].find('<') {
        let start = i + offset;
        let rest = &masked_spans[start..];
        if let Some(body) = rest.strip_prefix("<!--") {
            match body.find("-->") {
                Some(end) => {
                    let stop = start + 4 + end + 3;
                    blank(&mut out, start, stop);
                    i = stop;
                }
                None => {
                    blank(&mut out, start, raw.len());
                    *in_comment = true;
                    break;
                }
            }
            continue;
        }
        let autolink = ["<http://", "<https://", "<mailto:"]
            .iter()
            .any(|prefix| rest.starts_with(prefix));
        if autolink && let Some(end) = rest.find('>') {
            blank(&mut out, start, start + end + 1);
            i = start + end + 1;
            continue;
        }
        i = start + 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // covers: AC-9 (spec 0038)
    #[test]
    fn a_fence_hides_its_body_from_the_prose_and_keeps_its_language() {
        let doc = Markdown::parse("a\n```bash\ncargo xtask nope\n```\nb\n");
        assert_eq!(doc.fences.len(), 1);
        assert_eq!(doc.fences[0].lang, "bash");
        assert_eq!(doc.fences[0].body, vec![(3, "cargo xtask nope".to_owned())]);
        let prose: Vec<_> = doc.prose().collect();
        assert_eq!(prose, vec![(1, "a"), (5, "b")]);
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn an_indented_fence_inside_a_list_is_still_a_fence() {
        let doc = Markdown::parse("- item\n\n  ```text\n  TODO\n  ```\n");
        assert_eq!(doc.fences[0].lang, "text");
        assert_eq!(doc.fences[0].line, 3);
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn code_spans_autolinks_and_comments_are_blanked_in_the_prose() {
        let mut open = false;
        let line = "x `<data>` <https://example.com> <!-- TODO --> y";
        let masked = mask(line, &mut open);
        assert!(!masked.contains("data"));
        assert!(!masked.contains("example"));
        assert!(!masked.contains("TODO"));
        assert!(masked.starts_with('x') && masked.ends_with('y'));
        assert_eq!(masked.len(), line.len());
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn a_comment_over_two_lines_is_blanked_on_both() {
        let doc = Markdown::parse("<!-- start\nTODO -->\nafter\n");
        let prose: Vec<_> = doc
            .prose()
            .map(|(_, text)| text.trim().to_owned())
            .collect();
        assert_eq!(prose, vec!["", "", "after"]);
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn slugs_follow_the_github_rules_and_number_duplicates() {
        let doc = Markdown::parse("# Hello, World!\n## `cargo xtask docs`\n## Hello, World!\n");
        assert_eq!(
            doc.slugs(),
            vec!["hello-world", "cargo-xtask-docs", "hello-world-1"]
        );
    }

    // covers: AC-9 (spec 0038)
    #[test]
    fn code_spans_are_read_with_double_backticks() {
        assert_eq!(code_spans("a `x` and `` y` `` b"), vec!["x", "y`"]);
    }
}
