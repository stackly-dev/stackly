//! Text for people on stdout (spec 0008, *Output manusia*): [`Render`] and
//! [`HumanWriter`] with aligned label and value blocks, borderless tables,
//! state colors, local times, and ages. Only `--json` is a contract; this
//! text may change at any time.

use anstyle::Style;
use jiff::tz::TimeZone;
use serde::Serialize;
use stackly_protocol::Timestamp;
use unicode_width::UnicodeWidthStr;

use super::escape::escape_display;
use super::style::Tone;
use super::time::{format_age, format_local};

/// The text written for an empty cell or value.
pub const EMPTY: &str = "-";

/// A command result that can be written for people.
pub trait Render {
    /// Writes the result to `out`.
    fn render(&self, out: &mut HumanWriter);
}

/// No result, for a command that failed before it had one.
impl Render for () {
    fn render(&self, _out: &mut HumanWriter) {}
}

/// One value: text, already escaped when it came from the daemon or an
/// argument, and its style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    text: String,
    style: Style,
}

impl Cell {
    /// Text from the daemon or an argument, escaped. Empty text is `-`.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self::fixed(escape_display(text).into_owned())
    }

    /// Text the CLI wrote itself, such as a number or a formatted time.
    /// Empty text is `-`.
    #[must_use]
    pub fn fixed(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            text: if text.is_empty() {
                EMPTY.to_owned()
            } else {
                text
            },
            style: Style::new(),
        }
    }

    /// `text` from the daemon, or `-` when there is none.
    #[must_use]
    pub fn optional(text: Option<&str>) -> Self {
        Self::text(text.unwrap_or_default())
    }

    /// A wire value, such as a state, written with the color of `tone`.
    #[must_use]
    pub fn toned(text: &str, tone: Tone) -> Self {
        Self {
            style: tone.style(),
            ..Self::text(text)
        }
    }

    /// The cell's text without styling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    fn width(&self) -> usize {
        self.text.width()
    }
}

/// Builds the text of one result in memory, with or without color.
#[derive(Debug)]
pub struct HumanWriter {
    buf: String,
    color: bool,
    now: jiff::Timestamp,
    tz: TimeZone,
}

impl HumanWriter {
    /// A writer that colors when `color`, writes times in `tz`, and counts
    /// ages from `now`.
    #[must_use]
    pub const fn new(color: bool, now: jiff::Timestamp, tz: TimeZone) -> Self {
        Self {
            buf: String::new(),
            color,
            now,
            tz,
        }
    }

    /// The text written so far.
    #[must_use]
    pub fn into_string(self) -> String {
        self.buf
    }

    /// One line of the CLI's own text, written as it is.
    pub fn line(&mut self, text: &str) {
        self.buf.push_str(text);
        self.buf.push('\n');
    }

    /// One line of the CLI's own text with the color of `tone`.
    pub fn toned_line(&mut self, text: &str, tone: Tone) {
        let cell = Cell {
            text: text.to_owned(),
            style: tone.style(),
        };
        self.push_cell(&cell, 0);
        self.buf.push('\n');
    }

    /// A block of `label  value` lines, labels left aligned to the longest
    /// one plus two spaces. A row with an empty label continues the value
    /// above it.
    pub fn kv(&mut self, rows: &[(&str, Cell)]) {
        let width = rows
            .iter()
            .map(|(label, _)| label.width())
            .max()
            .unwrap_or(0);
        for (label, value) in rows {
            self.buf.push_str(label);
            self.pad(width - label.width() + 2);
            self.push_cell(value, 0);
            self.buf.push('\n');
        }
    }

    /// A borderless table: upper case headers, columns two spaces apart
    /// and as wide as their widest text, nothing cut.
    pub fn table(&mut self, headers: &[&str], rows: &[Vec<Cell>]) {
        self.indented_table(0, headers, rows);
    }

    /// [`Self::table`] with every line indented by `indent` spaces, for a
    /// list nested under a block.
    pub fn indented_table(&mut self, indent: usize, headers: &[&str], rows: &[Vec<Cell>]) {
        let headers: Vec<Cell> = headers
            .iter()
            .map(|header| Cell::fixed(header.to_uppercase()))
            .collect();
        let mut widths: Vec<usize> = headers.iter().map(Cell::width).collect();
        for row in rows {
            for (column, cell) in row.iter().enumerate() {
                if let Some(width) = widths.get_mut(column) {
                    *width = (*width).max(cell.width());
                }
            }
        }
        for row in std::iter::once(&headers).chain(rows) {
            self.pad(indent);
            let last = row.len().min(widths.len()).saturating_sub(1);
            for (column, (cell, width)) in row.iter().zip(&widths).enumerate() {
                let pad = if column == last {
                    0
                } else {
                    width - cell.width() + 2
                };
                self.push_cell(cell, pad);
            }
            self.buf.push('\n');
        }
    }

    /// `ts` in the system time zone, such as `2026-10-18 10:04:05 +07:00`.
    #[must_use]
    pub fn time(&self, ts: Timestamp) -> String {
        format_local(ts.as_jiff(), &self.tz)
    }

    /// `ts` with its age in brackets, such as
    /// `2026-10-18 10:04:05 +07:00 (3m ago)`.
    #[must_use]
    pub fn time_and_age(&self, ts: Timestamp) -> String {
        format!("{} ({})", self.time(ts), self.age(ts))
    }

    /// How long ago `ts` was, such as `3m ago`.
    #[must_use]
    pub fn age(&self, ts: Timestamp) -> String {
        format_age(ts.as_jiff(), self.now)
    }

    fn pad(&mut self, spaces: usize) {
        self.buf.extend(std::iter::repeat_n(' ', spaces));
    }

    fn push_cell(&mut self, cell: &Cell, pad: usize) {
        if self.color && cell.style != Style::new() {
            self.buf.push_str(&format!(
                "{}{}{}",
                cell.style.render(),
                cell.text,
                cell.style.render_reset()
            ));
        } else {
            self.buf.push_str(&cell.text);
        }
        self.pad(pad);
    }
}

/// The wire name of an enum value, such as `needs_recovery`; `unknown` for
/// a value that has none.
#[must_use]
pub fn wire_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use stackly_protocol::operation::OperationState;

    use super::*;

    fn writer(color: bool) -> HumanWriter {
        HumanWriter::new(
            color,
            "2026-10-18T03:07:05Z".parse().unwrap(),
            TimeZone::get("Asia/Jakarta").unwrap(),
        )
    }

    #[test]
    fn kv_aligns_values_two_spaces_after_the_longest_label() {
        let mut out = writer(false);
        out.kv(&[
            ("Status", Cell::fixed("ready")),
            ("Server ID", Cell::text("0192f3a0")),
            ("Started", Cell::optional(None)),
            ("", Cell::fixed("next line")),
        ]);
        assert_eq!(
            out.into_string(),
            "Status     ready\nServer ID  0192f3a0\nStarted    -\n           next line\n"
        );
    }

    #[test]
    fn tables_upper_case_headers_and_align_wide_text_without_cutting() {
        let mut out = writer(false);
        out.table(
            &["id", "name", "state"],
            &[
                vec![
                    Cell::fixed("1"),
                    Cell::text("数据库"),
                    Cell::fixed("running"),
                ],
                vec![Cell::fixed("22"), Cell::text("a"), Cell::optional(None)],
            ],
        );
        assert_eq!(
            out.into_string(),
            "ID  NAME    STATE\n1   数据库  running\n22  a       -\n"
        );
    }

    #[test]
    fn an_indented_table_indents_every_line() {
        let mut out = writer(false);
        out.indented_table(2, &["seq"], &[vec![Cell::fixed("1")]]);
        assert_eq!(out.into_string(), "  SEQ\n  1\n");
    }

    #[test]
    fn color_wraps_only_the_styled_text_and_never_the_padding() {
        let mut plain = writer(false);
        let mut colored = writer(true);
        for out in [&mut plain, &mut colored] {
            out.table(
                &["state", "id"],
                &[vec![
                    Cell::toned("failed", Tone::Critical),
                    Cell::fixed("1"),
                ]],
            );
        }
        assert_eq!(plain.into_string(), "STATE   ID\nfailed  1\n");
        assert_eq!(
            colored.into_string(),
            "STATE   ID\n\u{1b}[31mfailed\u{1b}[0m  1\n"
        );
    }

    #[test]
    fn daemon_text_is_escaped_and_measured_after_escaping() {
        let mut out = writer(true);
        out.table(
            &["name", "x"],
            &[vec![
                Cell::text("a\u{1b}[31mb\nc\u{202e}"),
                Cell::fixed("1"),
            ]],
        );
        let text = out.into_string();
        assert_eq!(
            text,
            "NAME                        X\na\\u{1b}[31mb\\u{a}c\\u{202e}  1\n"
        );
        assert!(!text.contains('\u{1b}'));
    }

    #[test]
    fn times_are_local_with_their_age() {
        let out = writer(false);
        let at: Timestamp = "2026-10-18T03:04:05Z".parse().unwrap();
        assert_eq!(out.time_and_age(at), "2026-10-18 10:04:05 +07:00 (3m ago)");
    }

    #[test]
    fn wire_names_come_from_serde() {
        assert_eq!(wire_name(&OperationState::NeedsRecovery), "needs_recovery");
        assert_eq!(wire_name(&7_u8), "unknown");
    }
}
