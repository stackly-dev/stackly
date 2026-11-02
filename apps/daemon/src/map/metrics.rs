//! Maps the metrics of `stackly-domain` to the DTOs of
//! `instance.get_metrics` (spec 0028): a measured number or its reason, the
//! usage of the main process or the tree, one sample, and one process row.
//! A value is copied as it is; a number that was not measured stays
//! unavailable with its reason and is never written as 0.

use stackly_domain::{
    Metric, MetricsSample as DomainSample, ProcessUsage, ResourceUsage as DomainUsage,
    UnavailableReason as DomainReason, UsageScope as DomainScope,
};
use stackly_protocol::instance::{
    CountMetric, DataUsageMetric, MetricsSample, NumberMetric, ProcessRow, ResourceUsage,
    ScannedUsage, UnavailableContent, UnavailableReason, UsageScope,
};
use stackly_protocol::{Count, Sequence, Timestamp};

use crate::detail::MeasuredUsage;

const fn reason(reason: DomainReason) -> UnavailableReason {
    match reason {
        DomainReason::WarmingUp => UnavailableReason::WarmingUp,
        DomainReason::NotPermitted => UnavailableReason::NotPermitted,
        DomainReason::ProcessExited => UnavailableReason::ProcessExited,
        DomainReason::NotSupported => UnavailableReason::NotSupported,
        DomainReason::NotScanned => UnavailableReason::NotScanned,
    }
}

const fn unavailable(why: DomainReason) -> UnavailableContent {
    UnavailableContent {
        reason: reason(why),
    }
}

/// A fractional metric, such as CPU percent.
#[must_use]
pub fn number(metric: &Metric<f64>) -> NumberMetric {
    match metric {
        Metric::Value(value) => NumberMetric::Value(*value),
        Metric::Unavailable(why) => NumberMetric::Unavailable(unavailable(*why)),
    }
}

/// A whole number metric, such as bytes or bytes per second.
#[must_use]
pub fn count(metric: &Metric<u64>) -> CountMetric {
    match metric {
        Metric::Value(value) => CountMetric::Value(Count::saturating(*value)),
        Metric::Unavailable(why) => CountMetric::Unavailable(unavailable(*why)),
    }
}

/// The usage of one scope in one sample.
#[must_use]
pub fn usage(usage: &DomainUsage) -> ResourceUsage {
    ResourceUsage {
        scope: match usage.scope {
            DomainScope::Main => UsageScope::Main,
            DomainScope::Tree => UsageScope::Tree,
        },
        process_count: Count::saturating(usage.process_count),
        measured_count: Count::saturating(usage.measured_count),
        cpu: number(&usage.cpu),
        memory: count(&usage.memory),
        disk_read: count(&usage.disk_read),
        disk_write: count(&usage.disk_write),
    }
}

/// One sample.
#[must_use]
pub fn sample(sample: &DomainSample) -> MetricsSample {
    MetricsSample {
        seq: Sequence::saturating(sample.seq),
        sampled_at: Timestamp::from_jiff(sample.sampled_at),
        main: usage(&sample.main),
        tree: usage(&sample.tree),
    }
}

/// One row of the process list.
#[must_use]
pub fn process_row(row: &ProcessUsage) -> ProcessRow {
    ProcessRow {
        pid: row.pid.get(),
        parent_pid: row.parent.map(|parent| parent.get()),
        depth: row.depth,
        name: row.name.clone(),
        is_main: row.is_main,
        cpu: number(&row.cpu),
        memory: count(&row.memory),
    }
}

/// The size of the data folder from the last scan, or `not_scanned`.
#[must_use]
pub fn data_usage(last_scan: Option<&MeasuredUsage>) -> DataUsageMetric {
    match last_scan {
        Some(scan) => DataUsageMetric::Scanned(ScannedUsage {
            bytes: Count::saturating(scan.bytes),
            complete: scan.complete,
            scanned_at: Timestamp::from_jiff(scan.scanned_at),
        }),
        None => DataUsageMetric::Unavailable(unavailable(DomainReason::NotScanned)),
    }
}
