//! The status groups of spec 0022 (*Kelompok status instance*), the same
//! table the app reads as `InstanceGroup`. A group reads only `lifecycle`
//! and `readiness.state`, never the process, the endpoints, or the restart
//! status, so an instance whose process lives but is not ready is never
//! counted as running.

use clap::ValueEnum;
use serde::Serialize;
use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState, InstanceSummary};

/// Which of the five groups an instance is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstanceGroup {
    /// Running and ready.
    Ready,
    /// Its process lives or is changing, but it is not ready.
    NotReady,
    /// Not running.
    Stopped,
    /// Initialization, a start, or the process failed.
    Failed,
    /// Its identity is lost.
    Unknown,
}

/// The group of `instance`, or `None` for a retained instance, which no
/// list shows.
#[must_use]
pub fn of(instance: &InstanceSummary) -> Option<InstanceGroup> {
    of_states(instance.lifecycle, instance.readiness.state)
}

/// The group of a lifecycle with a readiness state.
#[must_use]
pub const fn of_states(
    lifecycle: InstanceLifecycle,
    readiness: InstanceReadinessState,
) -> Option<InstanceGroup> {
    match lifecycle {
        InstanceLifecycle::Running => Some(match readiness {
            InstanceReadinessState::Ready => InstanceGroup::Ready,
            InstanceReadinessState::Unknown | InstanceReadinessState::NotReady => {
                InstanceGroup::NotReady
            }
        }),
        InstanceLifecycle::Initializing
        | InstanceLifecycle::Starting
        | InstanceLifecycle::Recovering
        | InstanceLifecycle::Stopping => Some(InstanceGroup::NotReady),
        InstanceLifecycle::Stopped | InstanceLifecycle::Uninitialized => {
            Some(InstanceGroup::Stopped)
        }
        InstanceLifecycle::Failed => Some(InstanceGroup::Failed),
        InstanceLifecycle::Unknown => Some(InstanceGroup::Unknown),
        InstanceLifecycle::Retained => None,
    }
}

/// `--status`: the same filters as the app's segmented control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFilter {
    /// `ready` and `notReady`.
    Running,
    /// `stopped`.
    Stopped,
    /// `failed`.
    Failed,
}

impl StatusFilter {
    /// Whether an instance in `group` passes this filter.
    #[must_use]
    pub const fn matches(self, group: InstanceGroup) -> bool {
        matches!(
            (self, group),
            (
                Self::Running,
                InstanceGroup::Ready | InstanceGroup::NotReady
            ) | (Self::Stopped, InstanceGroup::Stopped)
                | (Self::Failed, InstanceGroup::Failed)
        )
    }

    /// The word of the filter, as `--status` takes it.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

/// How many instances are in each group; `total` is their sum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceCounts {
    /// All five groups together.
    pub total: u64,
    /// Instances in the `ready` group.
    pub ready: u64,
    /// Instances in the `notReady` group.
    pub not_ready: u64,
    /// Instances in the `stopped` group.
    pub stopped: u64,
    /// Instances in the `failed` group.
    pub failed: u64,
    /// Instances in the `unknown` group.
    pub unknown: u64,
}

impl InstanceCounts {
    /// The counts of `instances`; a retained one is not counted.
    #[must_use]
    pub fn of(instances: &[InstanceSummary]) -> Self {
        let mut counts = Self::default();
        for group in instances.iter().filter_map(of) {
            counts.total += 1;
            match group {
                InstanceGroup::Ready => counts.ready += 1,
                InstanceGroup::NotReady => counts.not_ready += 1,
                InstanceGroup::Stopped => counts.stopped += 1,
                InstanceGroup::Failed => counts.failed += 1,
                InstanceGroup::Unknown => counts.unknown += 1,
            }
        }
        counts
    }

    /// `<total> instances: <ready> running, <stopped> stopped`, then
    /// `, <n> failed`, `, <n> not ready`, and `, <n> unknown` for each
    /// above 0.
    #[must_use]
    pub fn summary(&self) -> String {
        let noun = if self.total == 1 {
            "instance"
        } else {
            "instances"
        };
        let mut text = format!(
            "{} {noun}: {} running, {} stopped",
            self.total, self.ready, self.stopped
        );
        for (count, word) in [
            (self.failed, "failed"),
            (self.not_ready, "not ready"),
            (self.unknown, "unknown"),
        ] {
            if count > 0 {
                text.push_str(&format!(", {count} {word}"));
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Cases {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        lifecycle: InstanceLifecycle,
        readiness: InstanceReadinessState,
        group: Option<String>,
    }

    fn word(group: InstanceGroup) -> &'static str {
        match group {
            InstanceGroup::Ready => "ready",
            InstanceGroup::NotReady => "notReady",
            InstanceGroup::Stopped => "stopped",
            InstanceGroup::Failed => "failed",
            InstanceGroup::Unknown => "unknown",
        }
    }

    #[test]
    fn every_lifecycle_and_readiness_pair_lands_in_the_group_of_the_shared_fixture() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/instance/groups.json");
        let text = std::fs::read_to_string(&path).expect("the groups fixture reads");
        let fixture: Cases = serde_json::from_str(&text).expect("the groups fixture decodes");
        assert_eq!(fixture.cases.len(), 10 * 3);
        for case in fixture.cases {
            let group = of_states(case.lifecycle, case.readiness).map(word);
            assert_eq!(
                group,
                case.group.as_deref(),
                "{:?} with {:?}",
                case.lifecycle,
                case.readiness
            );
        }
    }

    #[test]
    fn a_status_filter_keeps_running_as_ready_plus_not_ready() {
        use InstanceGroup::{Failed, NotReady, Ready, Stopped, Unknown};
        let kept = |filter: StatusFilter| {
            [Ready, NotReady, Stopped, Failed, Unknown]
                .into_iter()
                .filter(|group| filter.matches(*group))
                .collect::<Vec<_>>()
        };
        assert_eq!(kept(StatusFilter::Running), [Ready, NotReady]);
        assert_eq!(kept(StatusFilter::Stopped), [Stopped]);
        assert_eq!(kept(StatusFilter::Failed), [Failed]);
    }

    #[test]
    fn the_count_line_names_only_the_groups_above_zero() {
        let counts = |total, ready, not_ready, stopped, failed, unknown| InstanceCounts {
            total,
            ready,
            not_ready,
            stopped,
            failed,
            unknown,
        };
        assert_eq!(
            counts(1, 0, 0, 1, 0, 0).summary(),
            "1 instance: 0 running, 1 stopped"
        );
        assert_eq!(
            counts(20, 4, 9, 4, 2, 1).summary(),
            "20 instances: 4 running, 4 stopped, 2 failed, 9 not ready, 1 unknown"
        );
    }
}
