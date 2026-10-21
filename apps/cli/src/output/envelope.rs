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

/// One record of a streaming command, before its final `result` record.
/// The first streaming command arrives with feature 27.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "feature 27 writes the first streaming command")
)]
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
/// # Errors
///
/// Returns the serializer's error, which a result of plain structs, strings,
/// and numbers never produces.
pub fn to_line<T: Serialize>(record: &T) -> serde_json::Result<Vec<u8>> {
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    Ok(line)
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
