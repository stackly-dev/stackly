//! The `--json` contract (spec 0008, *Kontrak `--json`*): one compact line
//! per record on stdout, fields in declaration order.

use std::io::{self, Write};

use serde::Serialize;
use stackly_protocol::ErrorBody;

/// The envelope version. A breaking change to the envelope or to a result
/// raises it, together with the spec, the goldens, and the tests.
pub const CLI_SCHEMA_VERSION: u32 = 2;

/// The warning codes, a fixed list outside the error catalog.
pub mod warning {
    /// `STACKLY_ROOT` is active; the message names the data root.
    pub const ROOT_OVERRIDE: &str = "cli.root_override";
    /// Same protocol major and minor, another contract digest.
    pub const CONTRACT_DRIFT: &str = "cli.contract_drift";
    /// Writing stdout failed for another reason than a broken pipe. Only
    /// ever written to stderr.
    pub const OUTPUT_FAILED: &str = "cli.output_failed";
    /// The daemon runs from another copy of the app than this `stackly`
    /// (spec 0015).
    pub const AGENT_BUNDLE_MISMATCH: &str = "cli.agent_bundle_mismatch";
    /// `install` worked, but no daemon answered within 30 seconds.
    pub const DAEMON_NOT_RUNNING: &str = "cli.daemon_not_running";
    /// `stop` saw a new daemon run: the daemon exited with an error and
    /// launchd started it again.
    pub const DAEMON_RELAUNCHED: &str = "cli.daemon_relaunched";
    /// `restart` did not see launchd start the daemon again and started it
    /// through macOS.
    pub const AGENT_RELAUNCHED: &str = "cli.agent_relaunched";
    /// `restart` saw an instance that was `unknown` before the restart and
    /// still is: its process was never touched (spec 0030).
    pub const RESTART_INSTANCE_UNKNOWN: &str = "cli.restart_instance_unknown";
    /// `instance autostart on` while the background agent is not enabled, so
    /// the instance will not start at login (spec 0031).
    pub const AGENT_OFF: &str = "cli.agent_off";
}

/// The final record of a command.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope<'a, T: Serialize> {
    /// [`CLI_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Always `result`.
    #[serde(rename = "type")]
    pub record: &'static str,
    /// The command path, such as `daemon status`; empty for `cli.usage`
    /// and for a panic before a command was chosen.
    pub command: &'a str,
    /// The command's result, serialized in its own field order.
    pub result: Option<&'a T>,
    /// The error, null exactly when the exit code is 0.
    pub error: Option<&'a ErrorBody>,
    /// The `requestId` of the daemon request that produced `error`.
    pub request_id: Option<&'a str>,
    /// The warnings, maybe none.
    pub warnings: &'a [Warning],
}

impl<'a, T: Serialize> Envelope<'a, T> {
    /// The `result` record of `command`.
    #[must_use]
    pub const fn new(
        command: &'a str,
        result: Option<&'a T>,
        error: Option<&'a ErrorBody>,
        request_id: Option<&'a str>,
        warnings: &'a [Warning],
    ) -> Self {
        Self {
            schema_version: CLI_SCHEMA_VERSION,
            record: "result",
            command,
            result,
            error,
            request_id,
            warnings,
        }
    }
}

/// One record of a streaming command, before its final `result` record;
/// `stackly instance logs` writes one per entry (spec 0027).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamItem<'a, T: Serialize> {
    /// [`CLI_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Always `item`.
    #[serde(rename = "type")]
    pub record: &'static str,
    /// The command path.
    pub command: &'a str,
    /// The streamed value.
    pub item: &'a T,
}

/// A warning: a code from [`warning`] and a message for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Warning {
    /// The warning code.
    pub code: String,
    /// One English sentence.
    pub message: String,
}

impl Warning {
    /// A warning with `code` and `message`.
    #[must_use]
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

/// `record` as one compact JSON line ending in `\n`.
///
/// Beside the C0 controls `serde_json` always escapes, DEL, the C1 controls,
/// and the bidi overrides and isolates inside a string are written as
/// `\uXXXX` (spec 0008, AC-3), so a line printed straight to a terminal
/// carries none of them raw. The JSON value is the same either way.
///
/// # Errors
///
/// Returns the serializer's error, which a result of plain structs, strings,
/// and numbers never produces.
pub fn to_line<T: Serialize>(record: &T) -> serde_json::Result<Vec<u8>> {
    let mut line = Vec::with_capacity(128);
    let mut serializer = serde_json::Serializer::with_formatter(&mut line, TerminalSafe);
    record.serialize(&mut serializer)?;
    line.push(b'\n');
    Ok(line)
}

/// The compact formatter, with the characters of
/// [`super::escape::needs_json_escape`] written as `\uXXXX`.
struct TerminalSafe;

impl serde_json::ser::Formatter for TerminalSafe {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (index, c) in fragment.char_indices() {
            if super::escape::needs_json_escape(c) {
                writer.write_all(&fragment.as_bytes()[start..index])?;
                write!(writer, "\\u{:04x}", u32::from(c))?;
                start = index + c.len_utf8();
            }
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

/// What happened when writing to stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// Everything was written.
    All,
    /// The reader went away (`| head -1`); writing stops without a word.
    BrokenPipe,
    /// Another error, which the caller reports as `cli.output_failed`.
    Failed(io::ErrorKind),
}

/// Writes `bytes` to `out` and flushes it.
pub fn write_all(out: &mut dyn Write, bytes: &[u8]) -> Written {
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Ok(()) => Written::All,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Written::BrokenPipe,
        Err(error) => Written::Failed(error.kind()),
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::codes::cli;

    use super::*;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Sample {
        zeta: u8,
        alpha: &'static str,
    }

    #[test]
    fn the_envelope_is_one_compact_line_with_fields_in_order() {
        let warnings = [Warning::new(warning::ROOT_OVERRIDE, "root")];
        let result = Sample {
            zeta: 1,
            alpha: "a",
        };
        let line = to_line(&Envelope::new(
            "daemon status",
            Some(&result),
            None,
            None,
            &warnings,
        ))
        .unwrap();
        assert_eq!(
            String::from_utf8(line).unwrap(),
            "{\"schemaVersion\":2,\"type\":\"result\",\"command\":\"daemon status\",\
             \"result\":{\"zeta\":1,\"alpha\":\"a\"},\"error\":null,\"requestId\":null,\
             \"warnings\":[{\"code\":\"cli.root_override\",\"message\":\"root\"}]}\n"
        );
    }

    #[test]
    fn control_characters_in_a_value_never_reach_the_line_raw() {
        let error = ErrorBody::new(&cli::PANIC, "a\u{1b}[31mb\r\nc");
        let line = to_line(&Envelope::<Sample>::new("", None, Some(&error), None, &[])).unwrap();
        assert!(
            !line[..line.len() - 1]
                .iter()
                .any(|b| matches!(b, 0x1b | b'\r' | b'\n'))
        );
    }

    #[test]
    fn del_c1_and_bidi_overrides_in_a_value_are_written_as_json_escapes() {
        let error = ErrorBody::new(&cli::PANIC, "a\u{7f}b\u{9b}31mc\u{202e}d\u{2069}e");
        let line = to_line(&Envelope::<Sample>::new("", None, Some(&error), None, &[])).unwrap();
        let text = String::from_utf8(line).unwrap();
        assert!(
            text.contains(r#""a\u007fb\u009b31mc\u202ed\u2069e""#),
            "{text}"
        );
        assert!(!text.contains(['\u{7f}', '\u{9b}', '\u{202e}', '\u{2069}']));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["error"]["message"],
            "a\u{7f}b\u{9b}31mc\u{202e}d\u{2069}e"
        );
    }

    #[test]
    fn bidi_marks_zero_width_characters_and_separators_in_a_value_are_written_as_json_escapes() {
        let message =
            "a\u{61c}b\u{200e}\u{200f}\u{200b}\u{200c}\u{200d}\u{2060}\u{feff}c\u{2028}\u{2029}d";
        let error = ErrorBody::new(&cli::PANIC, message);
        let line = to_line(&Envelope::<Sample>::new("", None, Some(&error), None, &[])).unwrap();
        let text = String::from_utf8(line).unwrap();
        assert!(
            text.contains(r#""a\u061cb\u200e\u200f\u200b\u200c\u200d\u2060\ufeffc\u2028\u2029d""#),
            "{text}"
        );
        assert!(!text.contains([
            '\u{61c}', '\u{200b}', '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{2028}',
            '\u{2029}', '\u{2060}', '\u{feff}'
        ]));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["error"]["message"], message);
    }

    #[test]
    fn printable_and_wide_text_stays_raw_in_the_line() {
        let item = StreamItem {
            schema_version: CLI_SCHEMA_VERSION,
            record: "item",
            command: "instance logs",
            item: &"café 数据 \u{a0}\"q\"",
        };
        assert_eq!(
            String::from_utf8(to_line(&item).unwrap()).unwrap(),
            "{\"schemaVersion\":2,\"type\":\"item\",\"command\":\"instance logs\",\
             \"item\":\"café 数据 \u{a0}\\\"q\\\"\"}\n"
        );
    }

    #[test]
    fn a_stream_item_names_its_record_type() {
        let item = StreamItem {
            schema_version: CLI_SCHEMA_VERSION,
            record: "item",
            command: "instance logs",
            item: &7,
        };
        assert_eq!(
            String::from_utf8(to_line(&item).unwrap()).unwrap(),
            "{\"schemaVersion\":2,\"type\":\"item\",\"command\":\"instance logs\",\"item\":7}\n"
        );
    }

    struct Failing(io::ErrorKind);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_broken_pipe_is_told_apart_from_other_write_errors() {
        let mut out = Vec::new();
        assert_eq!(write_all(&mut out, b"x\n"), Written::All);
        assert_eq!(out, b"x\n");
        assert_eq!(
            write_all(&mut Failing(io::ErrorKind::BrokenPipe), b"x"),
            Written::BrokenPipe
        );
        assert_eq!(
            write_all(&mut Failing(io::ErrorKind::PermissionDenied), b"x"),
            Written::Failed(io::ErrorKind::PermissionDenied)
        );
    }
}
