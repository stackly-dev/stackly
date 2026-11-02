//! The planned daemon restart (spec 0030): the reply of
//! `daemon.restart_preview`, the plan that `daemon.shutdown { mode:
//! restart }` writes, and the `daemon.restart_plan` event that carries it.
//!
//! A plan sorts every instance that has or waits for a process into `keep`
//! (the new daemon adopts it) or `stop_restore` (stopped before the old
//! daemon exits, started again by the new one). The desired state of an
//! instance is never part of a plan.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::daemon::LaunchContextKind;
use crate::instance::InstanceLifecycle;
use crate::operation::OperationKind;
use crate::wire::Timestamp;

/// Reply of `daemon.restart_preview`: the plan a restart asked for now
/// would write, and what would refuse it.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestartPreview {
    /// The sha256 hex digest of the plan's `instanceId:decision:reason`
    /// lines, sorted by instance ID; `daemon.shutdown` takes it back as
    /// `expectedPlanDigest`.
    pub digest: String,
    /// Who started the daemon; only `launchd` can restart.
    pub launch_context: LaunchContextKind,
    /// False while the registry is in recovery mode, so every instance is
    /// `keep` with `supervisor_unavailable`.
    pub supervisor_available: bool,
    /// Every instance in the plan, sorted by instance ID.
    #[serde(default)]
    pub instances: Vec<RestartPreviewInstance>,
    /// Queued and running operations; those with `resumable` false that
    /// still run after the quiesce wait refuse the restart.
    #[serde(default)]
    pub blocking_operations: Vec<BlockingOperation>,
}

/// One instance of a [`RestartPreview`].
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestartPreviewInstance {
    /// The instance.
    pub instance_id: Uuid,
    /// The instance name now.
    pub name: String,
    /// The provider name the instance is pinned to, such as `postgresql`.
    pub provider: String,
    /// The lifecycle the decision was made from.
    pub lifecycle: InstanceLifecycle,
    /// What the restart does with the instance.
    pub decision: PlanDecision,
    /// Why.
    pub reason: PlanReason,
}

/// An operation that may hold up a restart.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockingOperation {
    /// The operation.
    pub operation_id: Uuid,
    /// What it does.
    pub kind: OperationKind,
    /// Whether the next daemon resumes it; a resumable operation never
    /// refuses a restart.
    pub resumable: bool,
}

/// What a planned restart does with one instance. Open: newer daemons may
/// add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanDecision {
    /// Left alone; the new daemon adopts it.
    Keep,
    /// Stopped before the old daemon exits, started again by the new one.
    StopRestore,
    /// A decision this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Why an instance got its [`PlanDecision`]. Open: newer daemons may add
/// values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanReason {
    /// Its provider line has native evidence that its process survives a
    /// daemon restart.
    HandoverProven,
    /// Its provider line has no such evidence.
    HandoverUnproven,
    /// It waits for a respawn and has no process.
    RespawnPending,
    /// It is `unknown`, so it is never signalled.
    IdentityUnproven,
    /// The registry is in recovery mode, so nothing is stopped.
    SupervisorUnavailable,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// The state of a restart plan. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    /// The old daemon stops the `stop_restore` instances.
    Stopping,
    /// Every `stop_restore` instance stopped; the old daemon exits.
    HandedOver,
    /// The new daemon adopts and starts the instances again.
    Restoring,
    /// Every instance has an outcome; final.
    Finished,
    /// A stop timed out, so the old daemon kept running; final.
    Aborted,
    /// The Mac booted again before the plan finished; final.
    Expired,
    /// A state this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Why a plan closed without finishing. Open: newer daemons may add
/// values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanCloseReason {
    /// A `stop_restore` instance did not stop within its provider's stop
    /// timeout.
    StopTimeout,
    /// The new daemon runs in another boot session.
    BootChanged,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// What became of one instance of a plan. Open: newer daemons may add
/// values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanOutcome {
    /// The new daemon adopted its process.
    Adopted,
    /// It stopped and reached `ready` again.
    Restored,
    /// It did not end up running; see `failureCode`.
    Failed,
    /// It was removed, lost its data, or the plan closed first.
    Skipped,
    /// An outcome this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// A restart plan as clients see it: in `DaemonInfo.restart`, in the
/// `daemon.restart_plan` event, and in `stackly daemon restart --json`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestartPlanView {
    /// The plan ID, a lowercase UUID v7.
    pub id: Uuid,
    /// Where the plan is.
    pub state: PlanState,
    /// When the plan was written.
    pub created_at: Timestamp,
    /// When every `stop_restore` instance had stopped.
    #[serde(default)]
    pub handed_over_at: Option<Timestamp>,
    /// When the new daemon finished reconciling.
    #[serde(default)]
    pub reconciled_at: Option<Timestamp>,
    /// When the plan became final.
    #[serde(default)]
    pub finished_at: Option<Timestamp>,
    /// Why an `aborted` or `expired` plan closed.
    #[serde(default)]
    pub close_reason: Option<PlanCloseReason>,
    /// Every instance of the plan, sorted by instance ID.
    #[serde(default)]
    pub instances: Vec<RestartPlanInstance>,
}

/// One instance of a [`RestartPlanView`].
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestartPlanInstance {
    /// The instance; it may be gone since.
    pub instance_id: Uuid,
    /// The instance name when the plan was written.
    pub name: String,
    /// What the restart does with it.
    pub decision: PlanDecision,
    /// Why.
    pub reason: PlanReason,
    /// When the old daemon stopped it, for `stop_restore`.
    #[serde(default)]
    pub stopped_at: Option<Timestamp>,
    /// When it was `ready` again, or adopted.
    #[serde(default)]
    pub ready_at: Option<Timestamp>,
    /// What became of it, or null while the plan is open.
    #[serde(default)]
    pub outcome: Option<PlanOutcome>,
    /// The failure or blocker code of a `failed` outcome, such as
    /// `data_missing` or `identity_unproven`.
    #[serde(default)]
    pub failure_code: Option<String>,
}

/// Whether `text` is a plan digest: 64 lowercase hex digits.
#[must_use]
pub fn is_plan_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_this_build_does_not_know_decode_as_unrecognized() {
        let decision: PlanDecision = serde_json::from_str("\"pause\"").unwrap();
        assert_eq!(decision, PlanDecision::Unrecognized);
        let reason: PlanReason = serde_json::from_str("\"cold\"").unwrap();
        assert_eq!(reason, PlanReason::Unrecognized);
        let state: PlanState = serde_json::from_str("\"paused\"").unwrap();
        assert_eq!(state, PlanState::Unrecognized);
        let close: PlanCloseReason = serde_json::from_str("\"power\"").unwrap();
        assert_eq!(close, PlanCloseReason::Unrecognized);
        let outcome: PlanOutcome = serde_json::from_str("\"moved\"").unwrap();
        assert_eq!(outcome, PlanOutcome::Unrecognized);
    }

    #[test]
    fn a_plan_from_the_wire_needs_only_its_required_fields() {
        let plan: RestartPlanView = serde_json::from_value(serde_json::json!({
            "id": "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d30",
            "state": "stopping",
            "createdAt": "2026-11-02T08:00:00.000Z",
        }))
        .unwrap();
        assert_eq!(plan.state, PlanState::Stopping);
        assert!(plan.instances.is_empty());
        assert_eq!((plan.handed_over_at, plan.close_reason), (None, None));
        let json = serde_json::to_value(PlanDecision::StopRestore).unwrap();
        assert_eq!(json, "stop_restore");
    }

    #[test]
    fn a_digest_is_sixty_four_lowercase_hex_digits() {
        assert!(is_plan_digest(&"a1".repeat(32)));
        assert!(!is_plan_digest(&"A1".repeat(32)));
        assert!(!is_plan_digest(&"a1".repeat(31)));
        assert!(!is_plan_digest(&"g1".repeat(32)));
    }
}
