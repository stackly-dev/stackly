//! The redacting log writer (spec 0011 AC-13): every line of the daemon log
//! passes the shared [`Redactor`] after it was formatted, so the message,
//! the fields, and any error text are covered without each call site
//! remembering to.

use std::io::{self, Write};
use std::sync::Arc;

use stackly_core::secret::Redactor;
use tracing_subscriber::fmt::MakeWriter;

/// Wraps a [`MakeWriter`] so each event's line is redacted before the inner
/// writer sees it.
///
/// The `tracing` formatter writes an event into its writer in a few
/// `write` calls and drops the writer at the end of the event; this writer
/// collects them, redacts the whole line, and writes it to the inner writer
/// in one `write_all` when it drops. Redacting the whole line is what lets
/// a value that straddles two `write` calls still be found.
#[derive(Debug, Clone)]
pub struct RedactingMakeWriter<W> {
    inner: W,
    redactor: Arc<Redactor>,
}

impl<W> RedactingMakeWriter<W> {
    /// Redacts every line `inner` would write with `redactor`.
    #[must_use]
    pub const fn new(inner: W, redactor: Arc<Redactor>) -> Self {
        Self { inner, redactor }
    }
}

impl<'a, W> MakeWriter<'a> for RedactingMakeWriter<W>
where
    W: MakeWriter<'a>,
{
    type Writer = RedactingWriter<'a, W::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer(),
            redactor: &self.redactor,
            line: Vec::new(),
        }
    }
}

/// Collects one event's line and writes it, redacted, when dropped.
#[derive(Debug)]
pub struct RedactingWriter<'a, W: Write> {
    inner: W,
    redactor: &'a Redactor,
    line: Vec<u8>,
}

impl<W: Write> Write for RedactingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.line.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<W: Write> Drop for RedactingWriter<'_, W> {
    fn drop(&mut self) {
        if self.line.is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.line);
        // A line is JSON, so UTF-8; lossy only protects against a broken
        // writer, and a replaced byte is better than a leaked value.
        let text = String::from_utf8_lossy(&line);
        let redacted = self.redactor.redact(&text);
        // The inner writer queues the line when it drops, and a write error
        // has nowhere to go: the sink counts its own drops.
        let _ = self.inner.write_all(redacted.as_bytes());
        let _ = self.inner.flush();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use stackly_platform::SecretValue;

    use super::*;

    /// A writer factory that keeps every line it was given.
    #[derive(Debug, Clone, Default)]
    struct Lines(Arc<Mutex<Vec<String>>>);

    struct LineWriter(Arc<Mutex<Vec<String>>>, Vec<u8>);

    impl<'a> MakeWriter<'a> for Lines {
        type Writer = LineWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LineWriter(Arc::clone(&self.0), Vec::new())
        }
    }

    impl Write for LineWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.1.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Drop for LineWriter {
        fn drop(&mut self) {
            let text = String::from_utf8(std::mem::take(&mut self.1)).unwrap();
            self.0.lock().unwrap().push(text);
        }
    }

    fn redactor_knowing(value: &str) -> Arc<Redactor> {
        let redactor = Arc::new(Redactor::new());
        redactor.learn(&SecretValue::new(value.to_owned()).unwrap());
        redactor
    }

    #[test]
    fn a_known_value_is_hidden_even_when_the_formatter_writes_it_in_two_pieces() {
        let lines = Lines::default();
        let writer =
            RedactingMakeWriter::new(lines.clone(), redactor_knowing("fixture-secret-0001"));
        {
            let mut line = writer.make_writer();
            line.write_all(b"{\"message\":\"saw fixture-sec").unwrap();
            line.write_all(b"ret-0001 in a log\"}\n").unwrap();
        }
        let kept = lines.0.lock().unwrap().clone();
        assert_eq!(kept, ["{\"message\":\"saw **** in a log\"}\n"]);
    }

    #[test]
    fn the_patterns_hide_a_value_after_a_sensitive_key_without_a_known_value() {
        let lines = Lines::default();
        let writer = RedactingMakeWriter::new(lines.clone(), Arc::new(Redactor::new()));
        {
            let mut line = writer.make_writer();
            line.write_all(br#"{"message":"POSTGRES_PASSWORD=hunter2 postgres://app:pw@host/db"}"#)
                .unwrap();
        }
        let kept = lines.0.lock().unwrap().clone();
        assert!(
            !kept[0].contains("hunter2") && !kept[0].contains(":pw@"),
            "{kept:?}"
        );
        assert!(kept[0].contains("****"), "{kept:?}");
    }

    #[test]
    fn a_line_with_nothing_to_hide_passes_unchanged_and_an_empty_event_writes_nothing() {
        let lines = Lines::default();
        let writer = RedactingMakeWriter::new(lines.clone(), Arc::new(Redactor::new()));
        {
            let mut line = writer.make_writer();
            line.write_all(b"{\"message\":\"password authentication failed\"}\n")
                .unwrap();
        }
        drop(writer.make_writer());
        let kept = lines.0.lock().unwrap().clone();
        assert_eq!(
            kept[0],
            "{\"message\":\"password authentication failed\"}\n"
        );
        assert_eq!(kept.len(), 2, "the empty event still made an inner writer");
        assert_eq!(kept[1], "");
    }
}
