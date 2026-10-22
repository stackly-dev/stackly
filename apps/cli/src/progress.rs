//! Progress while waiting (spec 0008, *Progress*). Always on stderr through
//! [`StderrSink`]: one live line redrawn with `\r` and `ESC [ K` on a
//! terminal, one plain line per state or stage change elsewhere, nothing
//! with `--quiet`. stdout never sees progress.

use std::time::Duration;

use unicode_width::UnicodeWidthChar;

use crate::output::escape::escape_display;
use crate::output::stderr::StderrSink;
use crate::output::time::format_duration;

/// The display columns of the live line. The terminal width is not read,
/// since that needs `ioctl`.
pub const LIVE_WIDTH: usize = 60;

/// How progress is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    /// One line redrawn in place: stderr is a terminal and `TERM` is not
    /// `dumb`.
    Live,
    /// One plain line per state or stage change.
    Lines,
    /// Nothing: `--quiet`.
    Off,
}

impl ProgressMode {
    /// The mode for a stderr that is a terminal or not, a `TERM` that is
    /// `dumb` or not, and `--quiet`.
    #[must_use]
    pub const fn choose(stderr_is_tty: bool, term_is_dumb: bool, quiet: bool) -> Self {
        if quiet {
            Self::Off
        } else if stderr_is_tty && !term_is_dumb {
            Self::Live
        } else {
            Self::Lines
        }
    }
}

/// The words a person reads for the stage of an operation: the stages of a
/// create have a sentence each (spec 0020, *CLI*), every other stage its
/// wire name.
#[must_use]
pub fn stage_text(kind: stackly_protocol::operation::OperationKind, stage: &str) -> &str {
    use stackly_protocol::operation::OperationKind;
    if kind != OperationKind::CreateInstance {
        return stage;
    }
    match stage {
        "validate" => "Checking",
        "pin" => "Pinning provider package",
        "secrets" => "Storing secrets in Keychain",
        "layout" => "Creating folders",
        "register" => "Registering",
        "init" => "Initializing",
        other => other,
    }
}

/// One progress update, from an operation's state, stage, and progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Update<'a> {
    /// The state's wire value, or `unknown` before the first answer.
    pub state: &'a str,
    /// The stage, when the operation names one.
    pub stage: Option<&'a str>,
    /// `done` and `total`, only when the progress is counted.
    pub counted: Option<(u64, u64)>,
    /// How long the CLI has been waiting.
    pub waited: Duration,
}

/// Draws progress updates in one [`ProgressMode`].
#[derive(Debug)]
pub struct Progress {
    mode: ProgressMode,
    last: Option<(String, Option<String>)>,
}

impl Progress {
    /// Progress in `mode`.
    #[must_use]
    pub const fn new(mode: ProgressMode) -> Self {
        Self { mode, last: None }
    }

    /// The mode in use.
    #[must_use]
    pub const fn mode(&self) -> ProgressMode {
        self.mode
    }

    /// Shows `update`: redraws the live line, or writes a line when the
    /// state or the stage changed.
    pub fn show(&mut self, sink: &mut StderrSink, update: &Update<'_>) {
        match self.mode {
            ProgressMode::Off => {}
            ProgressMode::Live => {
                let mut text = update.state.to_owned();
                if let Some(stage) = update.stage {
                    text.push(' ');
                    text.push_str(stage);
                }
                if let Some((done, total)) = update.counted {
                    text.push_str(&format!(" {done}/{total}"));
                }
                text.push_str(&format!(" ({})", format_duration(update.waited)));
                sink.live(&fit(&escape_display(&text), LIVE_WIDTH));
            }
            ProgressMode::Lines => {
                let key = (update.state.to_owned(), update.stage.map(str::to_owned));
                if self.last.as_ref() == Some(&key) {
                    return;
                }
                let mut text = format!("{} {}", format_duration(update.waited), update.state);
                if let Some(stage) = update.stage {
                    text.push(' ');
                    text.push_str(stage);
                }
                sink.line(&escape_display(&text));
                self.last = Some(key);
            }
        }
    }

    /// Clears the live line before the final output.
    pub fn finish(&mut self, sink: &mut StderrSink) {
        sink.clear_live();
    }
}

/// `text` cut to at most `width` display columns.
fn fit(text: &str, width: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::stderr::Captured;

    fn sink() -> (StderrSink, Captured) {
        let captured = Captured::default();
        (StderrSink::new(Box::new(captured.clone()), false), captured)
    }

    fn update<'a>(state: &'a str, stage: Option<&'a str>, seconds: u64) -> Update<'a> {
        Update {
            state,
            stage,
            counted: None,
            waited: Duration::from_secs(seconds),
        }
    }

    #[test]
    fn the_mode_comes_from_the_terminal_term_and_quiet() {
        assert_eq!(ProgressMode::choose(true, false, false), ProgressMode::Live);
        assert_eq!(ProgressMode::choose(true, true, false), ProgressMode::Lines);
        assert_eq!(
            ProgressMode::choose(false, false, false),
            ProgressMode::Lines
        );
        assert_eq!(ProgressMode::choose(true, false, true), ProgressMode::Off);
        assert_eq!(ProgressMode::choose(false, true, true), ProgressMode::Off);
    }

    #[test]
    fn live_redraws_one_line_with_state_stage_count_and_wait() {
        let (mut sink, out) = sink();
        let mut progress = Progress::new(ProgressMode::Live);
        progress.show(
            &mut sink,
            &Update {
                counted: Some((2, 5)),
                ..update("running", Some("download"), 64)
            },
        );
        progress.show(&mut sink, &update("running", None, 65));
        progress.finish(&mut sink);
        assert_eq!(
            out.text(),
            "\r\u{1b}[Krunning download 2/5 (1m04s)\r\u{1b}[Krunning (1m05s)\r\u{1b}[K"
        );
    }

    #[test]
    fn the_live_line_is_escaped_and_cut_to_60_columns() {
        let (mut sink, out) = sink();
        let mut progress = Progress::new(ProgressMode::Live);
        let stage = format!("\u{1b}[31m{}", "数".repeat(40));
        progress.show(&mut sink, &update("running", Some(&stage), 1));
        let text = out.text();
        let line = text.strip_prefix("\r\u{1b}[K").unwrap();
        assert!(line.starts_with("running \\u{1b}[31m"));
        assert!(!line.contains('\u{1b}'));
        let width: usize = line.chars().map(|c| c.width().unwrap_or(0)).sum();
        assert!((LIVE_WIDTH - 1..=LIVE_WIDTH).contains(&width), "{width}");
    }

    #[test]
    fn lines_write_once_per_state_or_stage_change_without_cursor_codes() {
        let (mut sink, out) = sink();
        let mut progress = Progress::new(ProgressMode::Lines);
        for (state, stage, seconds) in [
            ("queued", None, 0),
            ("queued", None, 1),
            ("running", Some("fetch"), 2),
            ("running", Some("fetch"), 7),
            ("running", Some("unpack"), 12),
        ] {
            let mut counted = update(state, stage, seconds);
            counted.counted = Some((seconds, 100));
            progress.show(&mut sink, &counted);
        }
        progress.finish(&mut sink);
        assert_eq!(
            out.text(),
            "0s queued\n2s running fetch\n12s running unpack\n"
        );
    }

    #[test]
    fn off_writes_nothing() {
        let (mut sink, out) = sink();
        let mut progress = Progress::new(ProgressMode::Off);
        progress.show(&mut sink, &update("running", None, 1));
        progress.finish(&mut sink);
        assert_eq!(out.text(), "");
    }
}
