//! The log methods (spec 0027, *Method IPC*): `instance.logs_read`, the
//! reading side of `instance.logs_follow`, and the mapping of the durable
//! log store's pages to the `logs` DTOs.
//!
//! Every read runs on the blocking pool behind one semaphore of
//! [`LOG_READ_CONCURRENCY`] permits, so a burst of followers never takes
//! the threads the registry needs. A follow wakes when the writer reports a
//! new head, at most every [`LOG_FOLLOW_MIN_INTERVAL`], and reads the
//! entries after its cursor from disk; a slow client only falls behind on
//! disk.

use std::sync::Arc;
use std::time::Duration;

use stackly_core::logs::{
    DurableLog, FollowSource, FollowStep, LOG_SCAN_BUDGET, LineRecord, LogCursor as CoreCursor,
    LogEntry as CoreEntry, LogFilter, LogGap as CoreGap, LogHead, LogLevel as CoreLevel,
    LogPage as CorePage, LogPosition as CorePosition, LogReadError, LogStoreId, LogsHealth,
    ReadRequest, RecordKind, StoreFailure, StreamSet, read_page,
};
use stackly_domain::InstanceId;
use stackly_platform::OutputStream;
use stackly_protocol::codes::{instance, internal, request};
use stackly_protocol::daemon::{SubsystemHealth, SubsystemStatus, subsystem};
use stackly_protocol::json::FieldCode;
use stackly_protocol::limits::LOG_BATCH_MAX;
use stackly_protocol::logs::{
    LOG_READ_LIMIT_DEFAULT, LogCursor, LogEntry, LogEntryKind, LogExited, LogFollowEnd,
    LogFollowEndReason, LogFrame, LogGap, LogGapReason, LogLevel, LogLineEntry, LogPage,
    LogPosition, LogRetention, LogStarted, LogStream, LogStreamBytes, LogTruncated,
    LogsFollowParams, LogsFollowing, LogsReadParams,
};
use stackly_protocol::{Count, ErrorBody, FieldError, LogSeq, Timestamp};
use tokio::sync::{Semaphore, watch};
use tokio::time::Instant;

use crate::state::DaemonCore;

/// Log reads that run at once across the daemon.
pub const LOG_READ_CONCURRENCY: usize = 4;

/// The shortest time between two wakes of one follow.
pub const LOG_FOLLOW_MIN_INTERVAL: Duration = Duration::from_millis(250);

/// The bytes left for the frame or reply around the entries.
const FRAME_ROOM: usize = 2048;

/// The most entries one wake of a follow reads.
const FOLLOW_STEP_LIMIT: usize = 1000;

/// The durable log store of the run and the semaphore of its reads.
#[derive(Debug, Clone)]
pub struct LogService {
    log: DurableLog,
    reads: Arc<Semaphore>,
}

impl LogService {
    /// Wraps the store the start slot opened.
    #[must_use]
    pub fn new(log: DurableLog) -> Self {
        Self {
            log,
            reads: Arc::new(Semaphore::new(LOG_READ_CONCURRENCY)),
        }
    }

    /// The store.
    #[must_use]
    pub const fn log(&self) -> &DurableLog {
        &self.log
    }

    /// Runs `read` on the blocking pool once a read permit is free.
    async fn blocking<T: Send + 'static>(
        &self,
        read: impl FnOnce(&DurableLog) -> T + Send + 'static,
    ) -> Result<T, Box<ErrorBody>> {
        let _permit = Arc::clone(&self.reads)
            .acquire_owned()
            .await
            .map_err(|_| unexpected("the log reads are closed"))?;
        let log = self.log.clone();
        tokio::task::spawn_blocking(move || read(&log))
            .await
            .map_err(|_| unexpected("the log read task failed"))
    }

    fn request(&self, filter: LogFilter, position: CorePosition, limit: usize) -> ReadRequest {
        ReadRequest {
            filter,
            position,
            limit,
            byte_limit: LOG_BATCH_MAX as usize - FRAME_ROOM,
            scan_budget: LOG_SCAN_BUDGET,
            segments_per_stream: self.log.config().segments_per_stream,
        }
    }

    fn retention(&self) -> LogRetention {
        let config = self.log.config();
        LogRetention {
            segments_per_stream: Count::saturating(config.segments_per_stream as u64),
            segment_max_bytes: Count::saturating(config.segment_max_bytes),
        }
    }
}

/// The log store of the run, or the error a method answers before it is
/// open.
fn service_of(core: &DaemonCore) -> Result<LogService, Box<ErrorBody>> {
    core.supervision().supervisor()?;
    core.logs()
        .cloned()
        .ok_or_else(|| unexpected("the log store is not open"))
}

/// The real `instance.logs_read`.
///
/// # Errors
///
/// The errors of `instance.get` for an unknown instance,
/// `instance.logs_unreadable` when the store cannot be read, and
/// `internal.unexpected` when the read task fails.
pub async fn read(
    core: &DaemonCore,
    params: LogsReadParams,
) -> Result<Box<LogPage>, Box<ErrorBody>> {
    let service = service_of(core)?;
    let id = crate::ipc::instance::existing(core, params.instance).await?;
    let filter = filter(&params.streams, params.contains.as_deref());
    let position = match params.position.unwrap_or_default() {
        LogPosition::Newest => CorePosition::Newest,
        LogPosition::Before(anchor) => CorePosition::Before(core_cursor(anchor.cursor)),
        LogPosition::After(anchor) => CorePosition::After(core_cursor(anchor.cursor)),
    };
    let limit = params.limit.unwrap_or(LOG_READ_LIMIT_DEFAULT) as usize;
    let request = service.request(filter, position, limit);
    let page = service
        .blocking(move |log| read_page(log.platform().fs(), &log.dir(id), &request))
        .await?
        .map_err(|error| unreadable(&error))?;
    Ok(Box::new(page_dto(&page, service.retention())))
}

/// A follow whose `LogsFollowing` reply is about to go out.
pub struct LogFollow {
    subscription_id: u32,
    service: LogService,
    instance: InstanceId,
    source: FollowSource,
    heads: watch::Receiver<LogHead>,
    /// The last read stopped at its limit or budget: read again at once.
    more: bool,
    last_wake: Option<Instant>,
}

/// What a follow has for its connection next.
pub enum FollowNext {
    /// Frames to queue, in order.
    Frame(LogFrame),
    /// The last frame: the instance was deleted.
    Ended(LogFrame),
}

/// Prepares `instance.logs_follow` for the subscription `subscription_id`:
/// the receiver of the head is made first, then the head is read from
/// disk, so no entry falls between the reply and the first frame.
///
/// # Errors
///
/// The errors of [`read`], and `request.invalid_params` (field `after.seq`,
/// code `out_of_range`) for an `after` cursor of the current store beyond
/// its head, like `events.subscribe` (spec 0027 AC-7, audit note 124).
pub async fn follow(
    core: &DaemonCore,
    subscription_id: u32,
    params: LogsFollowParams,
) -> Result<(LogsFollowing, LogFollow), Box<ErrorBody>> {
    let service = service_of(core)?;
    let id = crate::ipc::instance::existing(core, params.instance).await?;
    let heads = service.log().subscribe(id);
    let request = service.request(LogFilter::all(), CorePosition::Newest, 1);
    let page = service
        .blocking(move |log| read_page(log.platform().fs(), &log.dir(id), &request))
        .await?
        .map_err(|error| unreadable(&error))?;
    let head = match (page.store, page.head) {
        (Some(store), Some(seq)) => Some(CoreCursor { store, seq }),
        _ => None,
    };
    if let (Some(after), Some(store)) = (params.after, page.store)
        && after.store_id == store.as_uuid()
        && after.seq.get() > page.head.unwrap_or(0)
    {
        let error = ErrorBody::new(&request::INVALID_PARAMS, "params do not match the method")
            .with_detail("method", "instance.logs_follow")
            .with_field(FieldError::new("after.seq", FieldCode::OutOfRange));
        return Err(Box::new(error));
    }
    let after = params.after.map(core_cursor).or(head);
    let following = LogsFollowing {
        subscription_id,
        store_id: page.store.map(|store| store.as_uuid()),
        head: head.map(cursor_dto),
    };
    let follow = LogFollow {
        subscription_id,
        service,
        instance: id,
        source: FollowSource::new(filter(&params.streams, params.contains.as_deref()), after),
        heads,
        more: true,
        last_wake: None,
    };
    Ok((following, follow))
}

impl LogFollow {
    /// The subscription the follow answers to.
    #[must_use]
    pub const fn subscription_id(&self) -> u32 {
        self.subscription_id
    }

    /// Waits for new entries and returns the next frame. A frame is never
    /// empty: it carries entries, a gap, or the end.
    pub async fn next(&mut self) -> FollowNext {
        loop {
            if !self.more {
                if !self.heads.borrow().removed && self.heads.changed().await.is_err() {
                    // The store went away with the daemon; nothing more comes.
                    std::future::pending::<()>().await;
                }
                if let Some(last) = self.last_wake {
                    tokio::time::sleep_until(last + LOG_FOLLOW_MIN_INTERVAL).await;
                }
            }
            self.last_wake = Some(Instant::now());
            self.heads.borrow_and_update();
            let removed = self.heads.borrow().removed;
            let step = self.step().await;
            let frame = match step {
                Ok(step) => {
                    self.more = step.more;
                    self.frame(step)
                }
                Err(error) => {
                    tracing::warn!(instance = %self.instance, code = error.code(), "a log follow could not read the store");
                    self.more = false;
                    None
                }
            };
            if removed && !self.more {
                let mut last = frame.unwrap_or_else(|| self.empty_frame());
                last.ended = Some(LogFollowEnd {
                    reason: LogFollowEndReason::InstanceRemoved,
                });
                return FollowNext::Ended(last);
            }
            if let Some(frame) = frame {
                return FollowNext::Frame(frame);
            }
        }
    }

    async fn step(&mut self) -> Result<FollowStep, Box<ErrorBody>> {
        let mut source = self.source.clone();
        let id = self.instance;
        let request = self
            .service
            .request(LogFilter::all(), CorePosition::Newest, 1);
        let (source, step) = self
            .service
            .blocking(move |log| {
                let step = source.step(
                    log.platform().fs(),
                    &log.dir(id),
                    FOLLOW_STEP_LIMIT,
                    request.byte_limit,
                    request.scan_budget,
                    request.segments_per_stream,
                );
                (source, step)
            })
            .await?;
        self.source = source;
        step.map_err(|error| unreadable(&error))
    }

    fn frame(&self, step: FollowStep) -> Option<LogFrame> {
        if step.entries.is_empty() && step.gap.is_none() {
            return None;
        }
        Some(LogFrame {
            subscription_id: self.subscription_id,
            store_id: self.source.cursor().map(|cursor| cursor.store.as_uuid()),
            entries: step.entries.iter().map(entry_dto).collect(),
            gap: step.gap.map(gap_dto),
            ended: None,
        })
    }

    fn empty_frame(&self) -> LogFrame {
        LogFrame {
            subscription_id: self.subscription_id,
            store_id: self.source.cursor().map(|cursor| cursor.store.as_uuid()),
            entries: Vec::new(),
            gap: None,
            ended: None,
        }
    }
}

/// The `instance_logs` subsystem of `daemon.get_health`.
#[must_use]
pub fn health(health: Option<LogsHealth>) -> SubsystemHealth {
    let (status, detail) = match health {
        None => (SubsystemStatus::NotBuilt, None),
        Some(health) if health.degraded => {
            let reason = health.reason.map_or("io", StoreFailure::as_code);
            let detail = if health.failing > 0 {
                format!(
                    "writing the log store fails for {} instances ({reason})",
                    health.failing
                )
            } else {
                format!("a write to the log store failed in the last minute ({reason})")
            };
            (SubsystemStatus::Degraded, Some(detail))
        }
        Some(_) => (SubsystemStatus::Ok, None),
    };
    SubsystemHealth {
        name: subsystem::INSTANCE_LOGS.to_owned(),
        status,
        detail,
    }
}

fn filter(streams: &[LogStream], contains: Option<&str>) -> LogFilter {
    let streams: Vec<OutputStream> = streams
        .iter()
        .filter_map(|stream| match stream {
            LogStream::Stdout => Some(OutputStream::Stdout),
            LogStream::Stderr => Some(OutputStream::Stderr),
            LogStream::Unrecognized => None,
        })
        .collect();
    LogFilter::new(StreamSet::of(&streams), contains)
}

fn core_cursor(cursor: LogCursor) -> CoreCursor {
    CoreCursor {
        store: LogStoreId::from_uuid(cursor.store_id),
        seq: cursor.seq.get(),
    }
}

fn cursor_dto(cursor: CoreCursor) -> LogCursor {
    LogCursor {
        store_id: cursor.store.as_uuid(),
        seq: LogSeq::saturating(cursor.seq),
    }
}

fn page_dto(page: &CorePage, retention: LogRetention) -> LogPage {
    let at = |seq: Option<u64>| {
        page.store
            .zip(seq)
            .map(|(store, seq)| cursor_dto(CoreCursor { store, seq }))
    };
    LogPage {
        store_id: page.store.map(|store| store.as_uuid()),
        entries: page.entries.iter().map(entry_dto).collect(),
        earlier: at(page.earlier),
        later: at(page.later),
        head: at(page.head),
        gap: page.gap.map(gap_dto),
        scan_limited: page.scan_limited,
        retention,
        stored_bytes: LogStreamBytes {
            stdout: Count::saturating(page.stored_bytes[0]),
            stderr: Count::saturating(page.stored_bytes[1]),
        },
        skipped_records: Count::saturating(page.skipped_records),
    }
}

/// Maps one entry of the store to its DTO.
#[must_use]
pub fn entry_dto(entry: &CoreEntry) -> LogEntry {
    LogEntry {
        seq: LogSeq::saturating(entry.seq),
        at: Timestamp::from(entry.at),
        stream: entry.stream.map(|stream| match stream {
            OutputStream::Stdout => LogStream::Stdout,
            OutputStream::Stderr => LogStream::Stderr,
        }),
        kind: kind_dto(&entry.kind),
    }
}

fn kind_dto(kind: &RecordKind) -> LogEntryKind {
    match kind {
        RecordKind::Line(LineRecord {
            text,
            level,
            partial,
            continued,
            step,
            ..
        }) => LogEntryKind::Line(LogLineEntry {
            text: text.clone(),
            level: match level {
                CoreLevel::Normal => LogLevel::Normal,
                CoreLevel::Warning => LogLevel::Warning,
                CoreLevel::Error => LogLevel::Error,
            },
            partial: *partial,
            continued: *continued,
            step: step.clone(),
        }),
        RecordKind::Truncated { bytes } => LogEntryKind::Truncated(LogTruncated {
            bytes: Count::saturating(*bytes),
        }),
        RecordKind::Started { pid } => LogEntryKind::Started(LogStarted { pid: *pid }),
        RecordKind::Exited { code, signal } => LogEntryKind::Exited(LogExited {
            code: *code,
            signal: *signal,
        }),
        RecordKind::Unavailable => LogEntryKind::Unavailable,
        _ => LogEntryKind::Unrecognized,
    }
}

const fn gap_dto(gap: CoreGap) -> LogGap {
    LogGap {
        reason: match gap {
            CoreGap::Rotated => LogGapReason::Rotated,
            CoreGap::StoreReset => LogGapReason::StoreReset,
            _ => LogGapReason::Unrecognized,
        },
    }
}

fn unreadable(error: &LogReadError) -> Box<ErrorBody> {
    let LogReadError::Unreadable(reason) = error else {
        return unexpected("the log store read failed");
    };
    Box::new(
        ErrorBody::new(
            &instance::LOGS_UNREADABLE,
            "the log store of the instance can't be read",
        )
        .with_detail("reason", reason.as_code()),
    )
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&internal::UNEXPECTED, message))
}
