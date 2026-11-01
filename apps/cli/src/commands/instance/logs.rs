//! `stackly instance logs` (spec 0027, *CLI*, AC-9 and AC-10): the last
//! entries of an instance's stored log, pages before or after a cursor,
//! and with `--follow` the entries that come after them.
//!
//! Every entry comes from `instance.logs_read` and `instance.logs_follow`,
//! the same methods the Logs screen uses, and is already redacted. Text
//! mode writes one line per entry as it arrives; `--json` writes one `item`
//! record per entry or gap, then one `result`. Ctrl+C during a follow ends
//! it with exit 0 and the `result`; during the paging before it, the
//! command stops with `cli.interrupted`, exit 130.

use clap::{Args, ValueEnum};
use jiff::tz::TimeZone;
use serde::Serialize;
use stackly_ipc::IpcError;
use stackly_protocol::instance::{GetInstanceParams, InstanceSummary};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::logs::{
    LOG_READ_LIMIT_MAX, LogAnchor, LogCursor, LogEntry, LogEntryKind, LogGap, LogGapReason,
    LogLevel, LogPage, LogPosition, LogRetention, LogStream, LogStreamBytes, LogsFollowParams,
    LogsReadParams, LogsUnfollowParams,
};
use stackly_protocol::{Call, Count, LogSeq, Reply};
use tokio::signal::unix::{Signal, SignalKind, signal};
use uuid::Uuid;

use super::resolve::{InstanceTarget, TargetArg};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::envelope::{CLI_SCHEMA_VERSION, StreamItem, Written, to_line, write_all};
use crate::output::escape::escape_display;
use crate::output::human::{HumanWriter, Render};
use crate::output::style::Tone;
use crate::output::{CommandOutput, OutputMode};
use crate::session::CallSpec;

/// The entries shown when `--tail` is left out.
pub const LOGS_CLI_TAIL_DEFAULT: u32 = 100;
/// The most entries `--tail` takes.
pub const LOGS_CLI_TAIL_MAX: u32 = 10_000;

/// Which streams to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StreamArg {
    /// Both streams and the markers.
    All,
    /// Standard output only.
    Stdout,
    /// Standard error only.
    Stderr,
}

/// A cursor on the command line: `<storeId>:<seq>`, as the JSON `last` and
/// the text of a cursor show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorArg(pub LogCursor);

impl std::str::FromStr for CursorArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let bad = || {
            "expected <storeId>:<seq>, such as 0192f3a5-0000-7000-8000-0000000000aa:4812".to_owned()
        };
        let (store, seq) = text.split_once(':').ok_or_else(bad)?;
        let store_id = Uuid::try_parse(store).map_err(|_| bad())?;
        let seq = seq
            .parse::<u64>()
            .ok()
            .and_then(LogSeq::new)
            .ok_or_else(bad)?;
        Ok(Self(LogCursor { store_id, seq }))
    }
}

/// `stackly instance logs`.
#[derive(Debug, Args)]
pub struct LogsArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// How many entries to show, 1 to 10000
    #[arg(long, value_name = "N", default_value_t = LOGS_CLI_TAIL_DEFAULT,
        value_parser = clap::value_parser!(u32).range(1..=i64::from(LOGS_CLI_TAIL_MAX)))]
    pub tail: u32,
    /// Keep showing new entries until Ctrl+C
    #[arg(long, short = 'f')]
    pub follow: bool,
    /// Which stream to show
    #[arg(long, value_enum, default_value = "all")]
    pub stream: StreamArg,
    /// Show only lines that hold this text, in any case
    #[arg(long, value_name = "TEXT")]
    pub grep: Option<String>,
    /// Show the entries before this cursor
    #[arg(long, value_name = "CURSOR", conflicts_with = "after")]
    pub before: Option<CursorArg>,
    /// Show the entries after this cursor
    #[arg(long, value_name = "CURSOR")]
    pub after: Option<CursorArg>,
}

/// The `result` of `stackly instance logs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsResult {
    /// The instance.
    pub instance: LogsInstance,
    /// The store; null before the instance wrote anything.
    pub store_id: Option<Uuid>,
    /// The cursor of the last entry shown; null when none was.
    pub last: Option<LogCursor>,
    /// How much the store keeps.
    pub retention: Option<LogRetention>,
    /// The bytes the store holds.
    pub stored_bytes: Option<LogStreamBytes>,
    /// Whole lines of the store that were not records, met by the reads.
    pub skipped_records: Count,
}

/// The instance of a [`LogsResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogsInstance {
    /// Its ID.
    pub id: Uuid,
    /// Its name.
    pub name: String,
}

/// The entries were already written as they came.
impl Render for LogsResult {
    fn render(&self, _out: &mut HumanWriter) {}
}

/// One `item` of `--json`.
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
enum LogsItem<'a> {
    Entry(&'a LogEntry),
    Gap(&'a LogGap),
}

impl RunCommand for LogsArgs {
    type Output = LogsResult;

    async fn run(self, cx: &mut Context) -> CommandOutput<LogsResult> {
        match run(self, cx).await {
            Ok(result) => CommandOutput::ok(result),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

/// What the command learned while it ran.
struct Seen {
    store_id: Option<Uuid>,
    last: Option<LogCursor>,
    retention: Option<LogRetention>,
    stored_bytes: Option<LogStreamBytes>,
    skipped: u64,
}

impl Seen {
    fn page(&mut self, page: &LogPage) {
        self.store_id = page.store_id.or(self.store_id);
        self.retention = Some(page.retention);
        self.stored_bytes = Some(page.stored_bytes);
        self.skipped += page.skipped_records.get();
    }
}

async fn run(args: LogsArgs, cx: &mut Context) -> Result<LogsResult, CliFailure> {
    if args.follow && args.before.is_some() {
        return Err(CliFailure::usage(
            "argument_conflict",
            "--follow shows what comes next, so it cannot start --before a cursor",
        ));
    }
    let mut interrupt = signal(SignalKind::interrupt()).ok();
    let (id, name) = interruptible(&mut interrupt, identify(&args.instance, cx)).await??;
    let streams = match args.stream {
        StreamArg::All => Vec::new(),
        StreamArg::Stdout => vec![LogStream::Stdout],
        StreamArg::Stderr => vec![LogStream::Stderr],
    };
    let mut printer = Printer::new(cx);
    let mut seen = Seen {
        store_id: None,
        last: None,
        retention: None,
        stored_bytes: None,
        skipped: 0,
    };
    let base = LogsReadParams {
        instance: id,
        streams: streams.clone(),
        contains: args.grep.clone(),
        position: None,
        limit: None,
    };
    let resume = interruptible(
        &mut interrupt,
        page_through(cx, &args, &base, &mut printer, &mut seen),
    )
    .await??;

    if args.follow && !printer.closed {
        follow(
            cx,
            &mut interrupt,
            &mut printer,
            &mut seen,
            &name,
            LogsFollowParams {
                instance: id,
                streams,
                contains: args.grep,
                after: resume,
            },
        )
        .await?;
    }
    Ok(LogsResult {
        instance: LogsInstance { id, name },
        store_id: seen.store_id,
        last: seen.last,
        retention: seen.retention,
        stored_bytes: seen.stored_bytes,
        skipped_records: Count::saturating(seen.skipped),
    })
}

/// Runs `work` unless Ctrl+C comes first, which stops the command with
/// `cli.interrupted`.
async fn interruptible<T>(
    interrupt: &mut Option<Signal>,
    work: impl Future<Output = T>,
) -> Result<T, CliFailure> {
    let Some(signal) = interrupt else {
        return Ok(work.await);
    };
    tokio::select! {
        biased;
        _ = signal.recv() => Err(CliFailure::interrupted_at("reading_logs")),
        done = work => Ok(done),
    }
}

/// The ID and the name of the instance.
async fn identify(target: &TargetArg, cx: &mut Context) -> Result<(Uuid, String), CliFailure> {
    let id = target.target.resolve(cx).await?;
    if let InstanceTarget::Name(name) = &target.target {
        return Ok((id, name.clone()));
    }
    let reply = cx
        .call(
            Call::InstanceGet(GetInstanceParams { instance_id: id }),
            CallSpec::control(),
        )
        .await?;
    match reply {
        Reply::InstanceGet(summary) => {
            let summary: InstanceSummary = *summary;
            Ok((id, summary.name))
        }
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

async fn read(cx: &mut Context, params: LogsReadParams) -> Result<LogPage, CliFailure> {
    match cx
        .call(Call::InstanceLogsRead(params), CallSpec::control())
        .await?
    {
        Reply::InstanceLogsRead(page) => Ok(*page),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// Reads and writes the entries `--tail`, `--before`, and `--after` ask
/// for, and returns the cursor a follow starts after.
async fn page_through(
    cx: &mut Context,
    args: &LogsArgs,
    base: &LogsReadParams,
    printer: &mut Printer,
    seen: &mut Seen,
) -> Result<Option<LogCursor>, CliFailure> {
    let want = args.tail as usize;
    if let Some(CursorArg(after)) = args.after {
        // Forward: the first `want` entries after the cursor, written as
        // each page arrives.
        let mut cursor = after;
        let mut shown = 0;
        loop {
            let mut params = base.clone();
            params.position = Some(LogPosition::After(LogAnchor { cursor }));
            params.limit = Some(limit(want - shown));
            let page = read(cx, params).await?;
            seen.page(&page);
            let take = page.entries.len().min(want - shown);
            printer.write(
                &mut cx.output.stdout,
                seen,
                page.gap.as_ref(),
                &page.entries[..take],
            );
            shown += take;
            if let Some(entry) = take.checked_sub(1).and_then(|last| page.entries.get(last)) {
                cursor = LogCursor {
                    store_id: page.store_id.unwrap_or(cursor.store_id),
                    seq: entry.seq,
                };
            }
            if shown >= want || printer.closed {
                return Ok(Some(cursor));
            }
            match page.later {
                Some(later) => cursor = later,
                // The page reached the head: a follow goes on after it.
                None => return Ok(page.head.or(Some(cursor))),
            }
        }
    }
    // Backward: pages before the cursor (or the newest), oldest written
    // first.
    let mut position = args
        .before
        .map_or(LogPosition::Newest, |CursorArg(cursor)| {
            LogPosition::Before(LogAnchor { cursor })
        });
    let mut collected: Vec<LogEntry> = Vec::new();
    let mut gap = None;
    let mut head = None;
    loop {
        let mut params = base.clone();
        params.position = Some(position);
        params.limit = Some(limit(want - collected.len()));
        let page = read(cx, params).await?;
        seen.page(&page);
        head = head.or(page.head);
        gap = gap.or(page.gap);
        let mut entries = page.entries;
        entries.append(&mut collected);
        collected = entries;
        match page.earlier {
            Some(earlier) if collected.len() < want => {
                position = LogPosition::Before(LogAnchor { cursor: earlier });
            }
            _ => break,
        }
    }
    let skip = collected.len().saturating_sub(want);
    printer.write(
        &mut cx.output.stdout,
        seen,
        gap.as_ref(),
        &collected[skip..],
    );
    Ok(head)
}

fn limit(left: usize) -> u32 {
    u32::try_from(left)
        .unwrap_or(LOG_READ_LIMIT_MAX)
        .clamp(1, LOG_READ_LIMIT_MAX)
}

/// Follows until Ctrl+C, the end of the follow, or a broken connection.
async fn follow(
    cx: &mut Context,
    interrupt: &mut Option<Signal>,
    printer: &mut Printer,
    seen: &mut Seen,
    name: &str,
    params: LogsFollowParams,
) -> Result<(), CliFailure> {
    let (following, mut stream) = interruptible(interrupt, cx.follow_logs(params)).await??;
    seen.store_id = following.store_id.or(seen.store_id);
    loop {
        let next = match interrupt {
            Some(signal) => tokio::select! {
                biased;
                _ = signal.recv() => None,
                next = stream.next() => Some(next),
            },
            None => Some(stream.next().await),
        };
        let Some(next) = next else {
            // Ctrl+C: the unfollow goes out without waiting for its answer.
            let unfollow = Call::InstanceLogsUnfollow(LogsUnfollowParams {
                subscription_id: following.subscription_id,
            });
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(200),
                cx.call_once(unfollow, CallSpec::control()),
            )
            .await;
            return Ok(());
        };
        match next {
            Ok(Some(DecodedFrame::Log(frame))) => {
                seen.store_id = frame.store_id.or(seen.store_id);
                printer.write(
                    &mut cx.output.stdout,
                    seen,
                    frame.gap.as_ref(),
                    &frame.entries,
                );
                if frame.ended.is_some() {
                    return Err(CliFailure::instance_not_found(name));
                }
                if printer.closed {
                    return Ok(());
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                return Err(CliFailure::daemon(
                    Box::new(IpcError::ConnectionClosed.to_error_body()),
                    None,
                ));
            }
            Err(body) => return Err(CliFailure::daemon(body, None)),
        }
    }
}

/// Writes entries to stdout as they come: lines in text mode, `item`
/// records with `--json`.
struct Printer {
    json: bool,
    color: bool,
    tz: TimeZone,
    command: String,
    /// Stdout went away (`| head`), or failed; nothing more is written.
    closed: bool,
}

impl Printer {
    fn new(cx: &Context) -> Self {
        Self {
            json: cx.output.mode == OutputMode::Json,
            color: cx.output.stdout_color,
            tz: cx.output.tz.clone(),
            command: cx.output.command.clone(),
            closed: false,
        }
    }

    fn write(
        &mut self,
        out: &mut dyn std::io::Write,
        seen: &mut Seen,
        gap: Option<&LogGap>,
        entries: &[LogEntry],
    ) {
        let mut bytes = Vec::new();
        if let Some(gap) = gap {
            bytes.extend(self.gap(gap));
        }
        for entry in entries {
            bytes.extend(self.entry(entry));
            if let Some(store_id) = seen.store_id {
                seen.last = Some(LogCursor {
                    store_id,
                    seq: entry.seq,
                });
            }
        }
        if bytes.is_empty() || self.closed {
            return;
        }
        if !matches!(write_all(out, &bytes), Written::All) {
            self.closed = true;
        }
    }

    fn item(&self, item: &LogsItem<'_>) -> Vec<u8> {
        to_line(&StreamItem {
            schema_version: CLI_SCHEMA_VERSION,
            record: "item",
            command: &self.command,
            item,
        })
        .unwrap_or_default()
    }

    fn gap(&self, gap: &LogGap) -> Vec<u8> {
        if self.json {
            return self.item(&LogsItem::Gap(gap));
        }
        let text = match gap.reason {
            LogGapReason::Rotated => "older lines were rotated out",
            LogGapReason::StoreReset => "the log store was reset",
            LogGapReason::Unrecognized => "some lines could not be read",
        };
        format!(
            "{:23}  {:7}  {}\n",
            "",
            "stackly",
            self.paint(text, Tone::Caution)
        )
        .into_bytes()
    }

    fn entry(&self, entry: &LogEntry) -> Vec<u8> {
        if self.json {
            return self.item(&LogsItem::Entry(entry));
        }
        let (text, tone) = text_of(entry);
        let source = match entry.stream {
            _ if !matches!(entry.kind, LogEntryKind::Line(_)) => "stackly",
            Some(LogStream::Stdout) => "stdout",
            Some(LogStream::Stderr) => "stderr",
            _ => "stackly",
        };
        let at = entry
            .at
            .as_jiff()
            .to_zoned(self.tz.clone())
            .strftime("%Y-%m-%d %H:%M:%S%.3f")
            .to_string();
        format!(
            "{at}  {source:7}  {}\n",
            self.paint(&escape_display(&text), tone)
        )
        .into_bytes()
    }

    fn paint(&self, text: &str, tone: Tone) -> String {
        if !self.color || tone == Tone::Neutral {
            return text.to_owned();
        }
        let style = tone.style();
        format!("{}{text}{}", style.render(), style.render_reset())
    }
}

/// The text of an entry and its tone.
fn text_of(entry: &LogEntry) -> (String, Tone) {
    match &entry.kind {
        LogEntryKind::Line(line) => {
            let tone = match line.level {
                LogLevel::Error => Tone::Critical,
                LogLevel::Warning => Tone::Caution,
                _ => Tone::Neutral,
            };
            (line.text.clone(), tone)
        }
        LogEntryKind::Started(started) => (
            format!("process started (pid {})", started.pid),
            Tone::Neutral,
        ),
        LogEntryKind::Exited(exited) => match (exited.code, exited.signal) {
            (Some(0), _) => ("process exited with code 0".to_owned(), Tone::Neutral),
            (Some(code), _) => (format!("process exited with code {code}"), Tone::Critical),
            (None, Some(signal)) => (
                format!("process exited with signal {signal}"),
                Tone::Critical,
            ),
            (None, None) => ("process exited".to_owned(), Tone::Neutral),
        },
        LogEntryKind::Truncated(truncated) => (
            format!("{} of output was dropped", bytes(truncated.bytes.get())),
            Tone::Caution,
        ),
        LogEntryKind::Unavailable => (
            "output while Stackly was not running could not be read".to_owned(),
            Tone::Caution,
        ),
        LogEntryKind::Unrecognized => (
            "an entry this version cannot show".to_owned(),
            Tone::Neutral,
        ),
    }
}

/// `bytes` for people: `512 bytes`, `1.5 KiB`, `1.0 MiB`, `2.0 GiB`.
fn bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a size for people needs one decimal"
    )]
    let value = bytes as f64;
    if value < KIB {
        format!("{bytes} bytes")
    } else if value < KIB * KIB {
        format!("{:.1} KiB", value / KIB)
    } else if value < KIB * KIB * KIB {
        format!("{:.1} MiB", value / (KIB * KIB))
    } else {
        format!("{:.1} GiB", value / (KIB * KIB * KIB))
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::logs::{LogExited, LogLineEntry, LogStarted, LogTruncated};

    use super::*;

    #[test]
    fn a_cursor_reads_as_the_store_and_the_seq() {
        let cursor: CursorArg = "0192f3a5-0000-7000-8000-0000000000aa:4812".parse().unwrap();
        assert_eq!(cursor.0.seq.get(), 4812);
        assert!("4812".parse::<CursorArg>().is_err());
        assert!("x:1".parse::<CursorArg>().is_err());
        assert!(
            "0192f3a5-0000-7000-8000-0000000000aa:-1"
                .parse::<CursorArg>()
                .is_err()
        );
    }

    #[test]
    fn sizes_read_in_binary_units() {
        assert_eq!(bytes(512), "512 bytes");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(1_048_576), "1.0 MiB");
    }

    const AT: &str = "2026-11-01T21:36:58.042Z";

    fn printer(tz: TimeZone, color: bool) -> Printer {
        Printer {
            json: false,
            color,
            tz,
            command: "instance logs".to_owned(),
            closed: false,
        }
    }

    fn entry(stream: Option<LogStream>, kind: LogEntryKind) -> LogEntry {
        LogEntry {
            seq: LogSeq::new(7).unwrap(),
            at: stackly_protocol::Timestamp::from_jiff(AT.parse().unwrap()),
            stream,
            kind,
        }
    }

    fn line(text: &str, level: LogLevel) -> LogEntryKind {
        LogEntryKind::Line(LogLineEntry {
            text: text.to_owned(),
            level,
            partial: false,
            continued: false,
            step: None,
        })
    }

    fn rendered(printer: &Printer, entry: &LogEntry) -> String {
        String::from_utf8(printer.entry(entry)).unwrap()
    }

    fn exited(code: Option<i32>, signal: Option<i32>) -> LogEntryKind {
        LogEntryKind::Exited(LogExited { code, signal })
    }

    // covers: AC-10 (local time in the CLI)
    #[test]
    fn a_line_shows_its_read_time_to_the_millisecond_in_the_local_zone() {
        let line = entry(
            Some(LogStream::Stderr),
            line("database system is ready", LogLevel::Normal),
        );

        let utc = rendered(&printer(TimeZone::UTC, false), &line);
        let jakarta = rendered(&printer(jiff::tz::offset(7).to_time_zone(), false), &line);

        assert_eq!(
            utc,
            "2026-11-01 21:36:58.042  stderr   database system is ready\n"
        );
        assert_eq!(
            jakarta,
            "2026-11-02 04:36:58.042  stderr   database system is ready\n"
        );
    }

    // covers: AC-5, AC-11 (markers show as rows whose source is stackly)
    #[test]
    fn markers_show_as_stackly_rows_with_their_own_text() {
        let printer = printer(TimeZone::UTC, false);
        let cases = [
            (
                LogEntryKind::Started(LogStarted { pid: 48213 }),
                "process started (pid 48213)",
            ),
            (exited(Some(0), None), "process exited with code 0"),
            (exited(Some(1), None), "process exited with code 1"),
            (exited(None, Some(9)), "process exited with signal 9"),
            (exited(None, None), "process exited"),
            (
                LogEntryKind::Truncated(LogTruncated {
                    bytes: Count::new(1536).unwrap(),
                }),
                "1.5 KiB of output was dropped",
            ),
            (
                LogEntryKind::Unavailable,
                "output while Stackly was not running could not be read",
            ),
        ];

        for (kind, text) in cases {
            let row = rendered(&printer, &entry(None, kind));
            assert_eq!(row, format!("2026-11-01 21:36:58.042  stackly  {text}\n"));
        }
    }

    #[test]
    fn a_marker_carried_on_a_stream_still_shows_stackly_as_its_source() {
        let row = rendered(
            &printer(TimeZone::UTC, false),
            &entry(
                Some(LogStream::Stdout),
                LogEntryKind::Truncated(LogTruncated {
                    bytes: Count::new(512).unwrap(),
                }),
            ),
        );

        assert!(row.contains("  stackly  512 bytes of output was dropped"));
    }

    // covers: AC-5 (gap rows for a rotated cursor and a reset store)
    #[test]
    fn gaps_show_as_stackly_rows_without_a_time() {
        let printer = printer(TimeZone::UTC, false);
        let rotated = LogGap {
            reason: LogGapReason::Rotated,
        };
        let reset = LogGap {
            reason: LogGapReason::StoreReset,
        };

        let rotated = String::from_utf8(printer.gap(&rotated)).unwrap();
        let reset = String::from_utf8(printer.gap(&reset)).unwrap();

        assert_eq!(
            rotated,
            format!("{:23}  stackly  older lines were rotated out\n", "")
        );
        assert_eq!(
            reset,
            format!("{:23}  stackly  the log store was reset\n", "")
        );
    }

    // covers: AC-8 (the level colors a line; stderr alone never does)
    #[test]
    fn only_warning_and_error_lines_and_failed_exits_are_painted() {
        let printer = printer(TimeZone::UTC, true);
        let painted =
            |kind| rendered(&printer, &entry(Some(LogStream::Stderr), kind)).contains('\u{1b}');

        assert!(!painted(line(
            "LOG:  checkpoint starting",
            LogLevel::Normal
        )));
        assert!(painted(line("WARNING:  low disk", LogLevel::Warning)));
        assert!(painted(line("FATAL:  bad login", LogLevel::Error)));
        assert!(!painted(exited(Some(0), None)));
        assert!(painted(exited(Some(1), None)));
        assert!(painted(exited(None, Some(9))));
    }

    #[test]
    fn control_characters_in_a_line_are_escaped_so_they_never_reach_the_terminal() {
        let row = rendered(
            &printer(TimeZone::UTC, false),
            &entry(
                Some(LogStream::Stdout),
                line("before\u{1b}[2Jafter\r", LogLevel::Normal),
            ),
        );

        assert!(!row.contains('\u{1b}'));
        assert!(!row.contains('\r'));
        assert!(row.contains("before\\u{1b}[2Jafter\\u{d}"));
    }
}
