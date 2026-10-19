//! [`StderrSink`], the one owner of stderr. Progress, warnings, `-v`
//! lines, hints, prompts, and errors all go through it, so the live
//! progress line is always cleared before anything else is written there
//! (spec 0008, AC-4).
//!
//! Progress writes its cursor codes to the raw handle, never through
//! `anstream`, which would strip the line clear code with colors off.

use std::fmt::Write as _;
use std::io::Write;

use anstyle::{AnsiColor, Style};
use stackly_protocol::ErrorBody;

use super::escape::escape_display;

/// Moves to the start of the line and clears it: `\r` then `ESC [ K`.
pub const CLEAR_LINE: &str = "\r\u{1b}[K";

/// The most `items` lines an error block shows.
const ITEMS_SHOWN: usize = 20;

/// Writes to stderr, best effort: a failed write to stderr has nowhere to
/// be reported.
pub struct StderrSink {
    out: Box<dyn Write + Send>,
    color: bool,
    live: bool,
}

impl std::fmt::Debug for StderrSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StderrSink")
            .field("color", &self.color)
            .field("live", &self.live)
            .finish_non_exhaustive()
    }
}

impl StderrSink {
    /// A sink over `out` that colors labels when `color`.
    #[must_use]
    pub fn new(out: Box<dyn Write + Send>, color: bool) -> Self {
        Self {
            out,
            color,
            live: false,
        }
    }

    /// Whether this sink colors its labels.
    #[must_use]
    pub const fn color(&self) -> bool {
        self.color
    }

    /// Clears the live progress line, if one is drawn.
    pub fn clear_live(&mut self) {
        if self.live {
            self.live = false;
            self.write(CLEAR_LINE);
        }
    }

    /// Redraws the live progress line with `text`, without a newline.
    pub fn live(&mut self, text: &str) {
        self.live = true;
        self.write(&format!("{CLEAR_LINE}{text}"));
    }

    /// Writes `text` and a newline after clearing the live line.
    pub fn line(&mut self, text: &str) {
        self.clear_live();
        self.write(&format!("{text}\n"));
    }

    /// Writes `text` without a newline after clearing the live line, for a
    /// prompt.
    pub fn prompt(&mut self, text: &str) {
        self.clear_live();
        self.write(text);
    }

    /// `label` in `style` when this sink colors.
    #[must_use]
    pub fn styled(&self, label: &str, style: Style) -> String {
        if self.color {
            format!("{}{label}{}", style.render(), style.render_reset())
        } else {
            label.to_owned()
        }
    }

    /// `warning: <message>`, the label yellow and bold.
    pub fn warning(&mut self, message: &str) {
        let label = self.styled("warning:", bold(AnsiColor::Yellow));
        self.line(&format!("{label} {}", escape_display(message)));
    }

    /// The error block of spec 0008: `error: <message>`, then indented
    /// `code:`, the body's `hint:`, one `hint:` per CLI hint, the fields,
    /// up to 20 items, and `request:` when `request_id` is given. `message`
    /// replaces the body's message when the CLI words it better for people
    /// (the body itself never changes). Daemon text is escaped.
    pub fn error(
        &mut self,
        body: &ErrorBody,
        message: Option<&str>,
        cli_hints: &[String],
        request_id: Option<&str>,
    ) {
        let label = self.styled("error:", bold(AnsiColor::Red));
        let mut text = format!(
            "{label} {}\n  code: {}\n",
            escape_display(message.unwrap_or(body.message())),
            escape_display(body.code())
        );
        if let Some(hint) = body.hint() {
            let _ = writeln!(text, "  hint: {}", escape_display(hint));
        }
        for hint in cli_hints {
            let _ = writeln!(text, "  hint: {}", escape_display(hint));
        }
        for field in body.fields() {
            let _ = writeln!(
                text,
                "  {}: {}",
                escape_display(field.path()),
                escape_display(field.message())
            );
        }
        for item in body.items().iter().take(ITEMS_SHOWN) {
            let pairs: Vec<String> = item
                .fields()
                .iter()
                .map(|field| {
                    format!(
                        "{}={}",
                        escape_display(field.key()),
                        escape_display(field.value())
                    )
                })
                .collect();
            let _ = writeln!(
                text,
                "  {}: {}",
                escape_display(item.kind()),
                pairs.join(" ")
            );
        }
        if body.items().len() > ITEMS_SHOWN {
            let _ = writeln!(text, "  ... and {} more", body.items().len() - ITEMS_SHOWN);
        }
        if let Some(request_id) = request_id {
            let _ = writeln!(text, "  request: {}", escape_display(request_id));
        }
        self.clear_live();
        self.write(&text);
    }

    fn write(&mut self, text: &str) {
        let _ = self
            .out
            .write_all(text.as_bytes())
            .and_then(|()| self.out.flush());
    }
}

/// `color` and bold.
fn bold(color: AnsiColor) -> Style {
    Style::new().bold().fg_color(Some(color.into()))
}

/// A writer tests read back, shared with the sink that owns a clone.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

#[cfg(test)]
impl Captured {
    /// Everything written so far, as text.
    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

#[cfg(test)]
impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::codes::{ipc, manifest, request};
    use stackly_protocol::json::FieldCode;
    use stackly_protocol::{ErrorItem, FieldError};

    use super::*;

    fn sink(color: bool) -> (StderrSink, Captured) {
        let captured = Captured::default();
        (StderrSink::new(Box::new(captured.clone()), color), captured)
    }

    #[test]
    fn the_live_line_is_cleared_before_any_other_write() {
        let (mut sink, out) = sink(false);
        sink.live("running 1/3 (2s)");
        sink.live("running 2/3 (3s)");
        sink.warning("careful");
        sink.line("-> operation.get r-1");
        sink.clear_live();
        assert_eq!(
            out.text(),
            "\r\u{1b}[Krunning 1/3 (2s)\r\u{1b}[Krunning 2/3 (3s)\r\u{1b}[Kwarning: careful\n\
             -> operation.get r-1\n"
        );
    }

    #[test]
    fn an_error_block_lists_code_hints_fields_items_and_the_request_with_v() {
        let (mut sink, out) = sink(false);
        let body = ErrorBody::new(&request::INVALID_PARAMS, "the params are invalid")
            .with_detail("method", "operation.list")
            .with_hint("check the flags")
            .with_field(FieldError::new("limit", FieldCode::OutOfRange));
        sink.error(
            &body,
            None,
            &["update stacklyd".to_owned()],
            Some("0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e"),
        );
        assert_eq!(
            out.text(),
            "error: the params are invalid\n  code: request.invalid_params\n  hint: check the flags\n\
             \x20 hint: update stacklyd\n  limit: value is out of range\n\
             \x20 request: 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e\n"
        );
    }

    #[test]
    fn at_most_twenty_items_are_shown_then_a_count_of_the_rest() {
        let (mut sink, out) = sink(false);
        let items = (0..25).map(|n| {
            ErrorItem::new("issue")
                .with_field("file", "manifest.yaml")
                .with_field("code", format!("c{n}"))
        });
        let body = ErrorBody::new(&manifest::INVALID, "the manifest is invalid").with_items(items);
        sink.error(&body, None, &[], None);
        let text = out.text();
        assert_eq!(
            text.matches("  issue: file=manifest.yaml code=c").count(),
            20
        );
        assert!(text.ends_with("  ... and 5 more\n"), "{text}");
        assert!(!text.contains("request:"));
    }

    #[test]
    fn labels_are_colored_only_when_the_sink_colors_and_daemon_text_is_escaped() {
        let body = ErrorBody::new(&ipc::IO_ERROR, "bad \u{1b}]0;title\u{7}");
        let (mut plain, plain_out) = sink(false);
        let (mut colored, colored_out) = sink(true);
        plain.error(&body, None, &[], None);
        colored.error(&body, None, &[], None);
        colored.warning("w");
        assert!(
            plain_out
                .text()
                .starts_with("error: bad \\u{1b}]0;title\\u{7}\n")
        );
        assert!(!plain_out.text().contains('\u{1b}'));
        assert!(
            colored_out
                .text()
                .starts_with("\u{1b}[1m\u{1b}[31merror:\u{1b}[0m bad")
        );
        assert!(
            colored_out
                .text()
                .contains("\u{1b}[1m\u{1b}[33mwarning:\u{1b}[0m w\n")
        );
    }

    #[test]
    fn the_cli_may_word_the_message_without_changing_the_body() {
        let (mut sink, out) = sink(false);
        let body = ErrorBody::new(&ipc::ENDPOINT_MISSING, "nothing exists at the socket path");
        sink.error(
            &body,
            Some("daemon is not running (no socket at /s)"),
            &[],
            None,
        );
        assert!(
            out.text()
                .starts_with("error: daemon is not running (no socket at /s)\n")
        );
        assert_eq!(body.message(), "nothing exists at the socket path");
    }
}
