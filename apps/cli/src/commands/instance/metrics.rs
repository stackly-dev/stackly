//! `stackly instance metrics` (spec 0028, *CLI*, AC-12): the newest sample
//! of an instance's CPU, memory, and Disk I/O for the main process and the
//! whole process tree, with the restarts since it was created, its last
//! failure, readiness, health, the size of its data folder, and the process
//! list.
//!
//! Everything comes from `instance.get_metrics`, the method the Metrics
//! screen uses. `--json` writes its reply as it is. With `--watch` the
//! command asks again every sample interval with the `historyId` and
//! `afterSeq` it holds and writes one line per new sample until Ctrl+C,
//! which ends it with exit 0 as `instance logs --follow` does. A number
//! that was not measured is written `unavailable (<reason>)`, never 0.

use std::time::Duration;

use clap::Args;
use jiff::tz::TimeZone;
use serde::Serialize;
use stackly_protocol::instance::{
    CountMetric, DataUsageMetric, GetMetricsParams, InstanceLastFailure, MetricsReply,
    MetricsSample, MetricsState, NumberMetric, ProcessRow, ResourceUsage, UnavailableReason,
    UsageScope,
};
use stackly_protocol::{Call, Reply};
use tokio::signal::unix::{Signal, SignalKind, signal};
use uuid::Uuid;

use super::render::{format_size, health_line, last_failure_line, readiness_line};
use super::resolve::TargetArg;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::OutputMode;
use crate::output::envelope::{CLI_SCHEMA_VERSION, StreamItem, Written, to_line, write_all};
use crate::output::escape::escape_display;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::time::format_duration;
use crate::session::CallSpec;

/// `stackly instance metrics`.
#[derive(Debug, Args)]
pub struct MetricsArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// Keep writing one line per new sample until Ctrl+C
    #[arg(long)]
    pub watch: bool,
}

/// The `result` of `stackly instance metrics`: the reply of
/// `instance.get_metrics` as it came, the last one when watching.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct MetricsOutput {
    reply: MetricsReply,
}

impl Render for MetricsOutput {
    fn render(&self, out: &mut HumanWriter) {
        render_metrics(out, &self.reply);
    }
}

/// One `item` of `--watch --json`.
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
enum WatchItem<'a> {
    Sample(&'a MetricsSample),
    HistoryReset,
    NotRunning(&'a MetricsReply),
}

impl RunCommand for MetricsArgs {
    type Output = MetricsOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<MetricsOutput> {
        match run(self, cx).await {
            Ok(reply) => CommandOutput::ok(MetricsOutput { reply }),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

async fn run(args: MetricsArgs, cx: &mut Context) -> Result<MetricsReply, CliFailure> {
    let id = args.instance.target.resolve(cx).await?;
    let first = read(cx, id, None, None).await?;
    if !args.watch {
        return Ok(first);
    }
    watch(cx, id, first).await
}

async fn read(
    cx: &mut Context,
    instance_id: Uuid,
    history_id: Option<String>,
    after_seq: Option<stackly_protocol::Sequence>,
) -> Result<MetricsReply, CliFailure> {
    let call = Call::InstanceGetMetrics(GetMetricsParams {
        instance_id,
        history_id,
        after_seq,
    });
    match cx.call(call, CallSpec::control()).await? {
        Reply::InstanceGetMetrics(reply) => Ok(*reply),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// Writes the lines of `--watch` as they come.
struct Watcher {
    json: bool,
    tz: TimeZone,
    command: String,
    /// Stdout went away (`| head`); nothing more is written.
    closed: bool,
}

impl Watcher {
    fn new(cx: &Context) -> Self {
        Self {
            json: cx.output.mode == OutputMode::Json,
            tz: cx.output.tz.clone(),
            command: cx.output.command.clone(),
            closed: false,
        }
    }

    fn write(&mut self, out: &mut dyn std::io::Write, bytes: Vec<u8>) {
        if bytes.is_empty() || self.closed {
            return;
        }
        if !matches!(write_all(out, &bytes), Written::All) {
            self.closed = true;
        }
    }

    fn item(&self, item: &WatchItem<'_>) -> Vec<u8> {
        to_line(&StreamItem {
            schema_version: CLI_SCHEMA_VERSION,
            record: "item",
            command: &self.command,
            item,
        })
        .unwrap_or_default()
    }

    fn sample(&self, sample: &MetricsSample) -> Vec<u8> {
        if self.json {
            return self.item(&WatchItem::Sample(sample));
        }
        let at = sample
            .sampled_at
            .as_jiff()
            .to_zoned(self.tz.clone())
            .strftime("%H:%M:%S")
            .to_string();
        format!(
            "{at}  main {}  tree {}\n",
            usage_line(&sample.main),
            usage_line(&sample.tree)
        )
        .into_bytes()
    }

    fn reset(&self) -> Vec<u8> {
        if self.json {
            self.item(&WatchItem::HistoryReset)
        } else {
            b"history reset\n".to_vec()
        }
    }

    fn not_running(&self, reply: &MetricsReply) -> Vec<u8> {
        if self.json {
            self.item(&WatchItem::NotRunning(reply))
        } else {
            format!("not running ({})\n", wire_name(&reply.lifecycle)).into_bytes()
        }
    }
}

/// Asks for new samples every interval until Ctrl+C, and returns the last
/// reply. The first reply's newest sample is written at once.
async fn watch(
    cx: &mut Context,
    id: Uuid,
    first: MetricsReply,
) -> Result<MetricsReply, CliFailure> {
    let mut interrupt = signal(SignalKind::interrupt()).ok();
    let mut watcher = Watcher::new(cx);
    let mut last = first;
    let mut history_id: Option<String> = None;
    let mut after: Option<stackly_protocol::Sequence> = None;
    let mut running = true;
    let mut first_reply = true;
    loop {
        match last.state {
            MetricsState::Live => {
                running = true;
                // A process that has no sample yet answers the cursor of the
                // old one with a reset and no samples; the cursor moves on
                // below, so this reply is the only one that says so.
                if last.history_reset && !first_reply {
                    let bytes = watcher.reset();
                    watcher.write(&mut cx.output.stdout, bytes);
                }
                // The first reply holds the whole history; only its newest
                // sample is news.
                let fresh = if first_reply {
                    last.samples.last().into_iter().collect()
                } else {
                    last.samples.iter().collect::<Vec<_>>()
                };
                for sample in fresh {
                    let bytes = watcher.sample(sample);
                    watcher.write(&mut cx.output.stdout, bytes);
                }
                history_id = last.history_id.clone().or(history_id);
                after = last.samples.last().map(|sample| sample.seq).or(after);
            }
            _ if running => {
                running = false;
                history_id = None;
                after = None;
                let bytes = watcher.not_running(&last);
                watcher.write(&mut cx.output.stdout, bytes);
            }
            _ => {}
        }
        first_reply = false;
        if watcher.closed {
            return Ok(last);
        }
        let wait = Duration::from_secs(u64::from(last.interval_seconds.max(1)));
        if sleep_or_interrupt(&mut interrupt, wait).await {
            return Ok(last);
        }
        last = match interruptible(&mut interrupt, read(cx, id, history_id.clone(), after)).await {
            Some(reply) => reply?,
            None => return Ok(last),
        };
    }
}

/// Waits `wait`; true when Ctrl+C came first.
async fn sleep_or_interrupt(interrupt: &mut Option<Signal>, wait: Duration) -> bool {
    let Some(signal) = interrupt else {
        tokio::time::sleep(wait).await;
        return false;
    };
    tokio::select! {
        biased;
        _ = signal.recv() => true,
        () = tokio::time::sleep(wait) => false,
    }
}

/// Runs `work` unless Ctrl+C comes first, which gives `None`.
async fn interruptible<T>(
    interrupt: &mut Option<Signal>,
    work: impl Future<Output = T>,
) -> Option<T> {
    let Some(signal) = interrupt else {
        return Some(work.await);
    };
    tokio::select! {
        biased;
        _ = signal.recv() => None,
        done = work => Some(done),
    }
}

/// `unavailable (<reason>)` for a number that was not measured.
fn unavailable(reason: UnavailableReason) -> String {
    let name = match reason {
        UnavailableReason::WarmingUp => "warming_up",
        UnavailableReason::NotPermitted => "not_permitted",
        UnavailableReason::ProcessExited => "process_exited",
        UnavailableReason::NotSupported => "not_supported",
        UnavailableReason::NotScanned => "not_scanned",
        UnavailableReason::Unrecognized => "unrecognized",
    };
    format!("unavailable ({name})")
}

/// A CPU percent such as `12.5%`.
fn cpu_text(metric: &NumberMetric) -> String {
    match metric {
        NumberMetric::Value(percent) => format!("{percent:.1}%"),
        NumberMetric::Unavailable(content) => unavailable(content.reason),
        NumberMetric::Unrecognized => unavailable(UnavailableReason::Unrecognized),
    }
}

/// Bytes such as `78.0 MB`, with `/s` for a rate.
fn bytes_text(metric: &CountMetric, rate: bool) -> String {
    match metric {
        CountMetric::Value(bytes) => {
            let text = format_size(bytes.get());
            if rate { format!("{text}/s") } else { text }
        }
        CountMetric::Unavailable(content) => unavailable(content.reason),
        CountMetric::Unrecognized => unavailable(UnavailableReason::Unrecognized),
    }
}

/// `cpu 0.4% mem 78.0 MB` for the line of a watched sample.
fn usage_line(usage: &ResourceUsage) -> String {
    format!(
        "cpu {} mem {}",
        cpu_text(&usage.cpu),
        bytes_text(&usage.memory, false)
    )
}

/// The label of a scope: which processes the numbers cover.
fn scope_label(usage: &ResourceUsage, pid: Option<u32>) -> String {
    match usage.scope {
        UsageScope::Main => pid.map_or_else(
            || "main process".to_owned(),
            |pid| format!("main process (pid {pid})"),
        ),
        _ => {
            let mut text = format!(
                "process tree ({} processes, summed per process)",
                usage.process_count.get()
            );
            if usage.measured_count < usage.process_count {
                text.push_str(&format!(", {} measured", usage.measured_count.get()));
            }
            text
        }
    }
}

fn render_metrics(out: &mut HumanWriter, reply: &MetricsReply) {
    if reply.state != MetricsState::Live {
        out.line(&format!("not running ({})", wire_name(&reply.lifecycle)));
        return;
    }
    let mut rows: Vec<(&str, Cell)> = Vec::new();
    let newest = reply.samples.last();
    if let Some(process) = &reply.process {
        rows.push((
            "Process",
            Cell::fixed(format!(
                "pid {}, started {}",
                process.pid,
                out.time_and_age(process.started_at)
            )),
        ));
        if let Some(sample) = newest {
            let up = sample
                .sampled_at
                .as_jiff()
                .duration_since(process.started_at.as_jiff());
            rows.push(("Uptime", Cell::fixed(format_duration(up.unsigned_abs()))));
        }
    }
    rows.push((
        "Restarts",
        Cell::fixed(format!("{} since created", reply.restart_total.get())),
    ));
    rows.push((
        "Last failure",
        Cell::text(
            &reply
                .last_failure
                .as_ref()
                .map_or_else(String::new, |f| failure_text(out, f)),
        ),
    ));
    rows.push((
        "Readiness",
        Cell::text(&with_latency(
            readiness_line(out, &reply.readiness),
            reply.readiness.latency_ms,
        )),
    ));
    rows.push((
        "Health",
        Cell::text(&with_latency(
            health_line(out, &reply.health),
            reply.health.latency_ms,
        )),
    ));
    rows.push(("Data size", Cell::fixed(data_text(out, &reply.data_usage))));
    rows.push((
        "Sampled",
        Cell::fixed(match newest {
            Some(sample) => format!(
                "sample {} at {}, every {} s, {} kept",
                sample.seq.get(),
                out.time(sample.sampled_at),
                reply.interval_seconds,
                reply.capacity.get()
            ),
            None => "waiting for the first sample".to_owned(),
        }),
    ));
    out.kv(&rows);
    let Some(sample) = newest else {
        return;
    };
    let pid = reply.process.as_ref().map(|process| process.pid);
    for usage in [&sample.main, &sample.tree] {
        out.line("");
        out.line(&scope_label(usage, pid));
        out.kv(&[
            ("  CPU", Cell::fixed(cpu_text(&usage.cpu))),
            ("  Memory", Cell::fixed(bytes_text(&usage.memory, false))),
            (
                "  Disk read",
                Cell::fixed(bytes_text(&usage.disk_read, true)),
            ),
            (
                "  Disk write",
                Cell::fixed(bytes_text(&usage.disk_write, true)),
            ),
        ]);
    }
    if !reply.latest_processes.is_empty() {
        out.line("");
        let rows: Vec<Vec<Cell>> = reply.latest_processes.iter().map(process_row).collect();
        out.table(&["pid", "parent", "name", "memory", "cpu"], &rows);
        if reply.processes_truncated {
            out.line("more processes are not shown");
        }
    }
}

fn process_row(row: &ProcessRow) -> Vec<Cell> {
    let indent = "  ".repeat(usize::try_from(row.depth).unwrap_or(0).min(16));
    let name = format!("{indent}{}", escape_display(&row.name));
    vec![
        Cell::fixed(row.pid.to_string()),
        Cell::fixed(
            row.parent_pid
                .map_or_else(String::new, |pid| pid.to_string()),
        ),
        Cell::fixed(name),
        Cell::fixed(bytes_text(&row.memory, false)),
        Cell::fixed(cpu_text(&row.cpu)),
    ]
}

fn failure_text(out: &HumanWriter, failure: &InstanceLastFailure) -> String {
    last_failure_line(out, failure)
}

fn with_latency(text: String, latency_ms: Option<u32>) -> String {
    match latency_ms {
        Some(ms) => format!("{text}, {ms} ms"),
        None => text,
    }
}

/// `<size>, scanned <time>`, `at least <size>, scanned <time>` for a
/// partial scan, or `unavailable (not_scanned)`.
fn data_text(out: &HumanWriter, usage: &DataUsageMetric) -> String {
    match usage {
        DataUsageMetric::Scanned(scan) => {
            let size = format_size(scan.bytes.get());
            let size = if scan.complete {
                size
            } else {
                format!("at least {size}")
            };
            format!("{size}, scanned {}", out.time(scan.scanned_at))
        }
        DataUsageMetric::Unavailable(content) => unavailable(content.reason),
        DataUsageMetric::Unrecognized => unavailable(UnavailableReason::Unrecognized),
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::Count;
    use stackly_protocol::instance::UnavailableContent;

    use super::*;

    fn usage(scope: UsageScope, count: u64, measured: u64) -> ResourceUsage {
        ResourceUsage {
            scope,
            process_count: Count::saturating(count),
            measured_count: Count::saturating(measured),
            cpu: NumberMetric::Value(12.34),
            memory: CountMetric::Value(Count::saturating(78_000_000)),
            disk_read: CountMetric::Unavailable(UnavailableContent {
                reason: UnavailableReason::WarmingUp,
            }),
            disk_write: CountMetric::Value(Count::saturating(0)),
        }
    }

    #[test]
    fn an_unmeasured_number_reads_unavailable_with_its_reason_never_zero() {
        let missing = usage(UsageScope::Main, 1, 1).disk_read;

        assert_eq!(bytes_text(&missing, true), "unavailable (warming_up)");
        assert_eq!(
            bytes_text(&CountMetric::Unrecognized, false),
            "unavailable (unrecognized)"
        );
        assert_eq!(cpu_text(&NumberMetric::Value(0.0)), "0.0%");
        assert_eq!(
            cpu_text(&NumberMetric::Unavailable(UnavailableContent {
                reason: UnavailableReason::NotPermitted
            })),
            "unavailable (not_permitted)"
        );
    }

    #[test]
    fn rates_carry_a_per_second_unit_and_sizes_do_not() {
        let value = CountMetric::Value(Count::saturating(1_500));

        assert_eq!(bytes_text(&value, false), "1.5 kB");
        assert_eq!(bytes_text(&value, true), "1.5 kB/s");
    }

    #[test]
    fn the_scope_label_says_how_many_processes_were_measured() {
        assert_eq!(
            scope_label(&usage(UsageScope::Main, 1, 1), Some(4242)),
            "main process (pid 4242)"
        );
        assert_eq!(
            scope_label(&usage(UsageScope::Tree, 6, 6), Some(4242)),
            "process tree (6 processes, summed per process)"
        );
        assert_eq!(
            scope_label(&usage(UsageScope::Tree, 6, 5), Some(4242)),
            "process tree (6 processes, summed per process), 5 measured"
        );
    }

    #[test]
    fn a_watched_sample_is_one_line_with_both_scopes() {
        let watcher = Watcher {
            json: false,
            tz: TimeZone::UTC,
            command: "instance metrics".to_owned(),
            closed: false,
        };
        let sample = MetricsSample {
            seq: stackly_protocol::Sequence::new(3).expect("a sequence"),
            sampled_at: stackly_protocol::Timestamp::from_jiff(
                "2026-11-02T09:00:15Z".parse().expect("a time"),
            ),
            main: usage(UsageScope::Main, 1, 1),
            tree: usage(UsageScope::Tree, 2, 2),
        };

        let line = String::from_utf8(watcher.sample(&sample)).expect("text");

        assert_eq!(
            line,
            "09:00:15  main cpu 12.3% mem 78.0 MB  tree cpu 12.3% mem 78.0 MB\n"
        );
    }
}
