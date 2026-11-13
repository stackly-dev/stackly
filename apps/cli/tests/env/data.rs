//! The environment DTOs the scripted daemon answers with: the wire
//! fixtures of `protocols/fixtures/env/`, and the shapes they lack made
//! from the same DTOs.

use std::path::PathBuf;

use stackly_protocol::env::{
    EnvExport, EnvLockResult, EnvPlan, EnvSource, EnvValidation, EnvironmentDetail,
    EnvironmentList, RemovedEnvironment,
};
use stackly_protocol::{Call, Count, Reply, Revision};

use crate::common::scripted::{Ctx, Step, error};

fn read(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/env")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} reads: {error}", path.display()))
}

/// `validation-invalid.json`: two error findings.
pub fn invalid() -> EnvValidation {
    serde_json::from_str(&read("validation-invalid.json")).expect("the validation decodes")
}

/// `plan-applicable.json`: one create item with three steps and one keep.
pub fn applicable() -> EnvPlan {
    serde_json::from_str(&read("plan-applicable.json")).expect("the plan decodes")
}

/// `plan-blocked.json`: the same items with two blockers.
pub fn blocked() -> EnvPlan {
    serde_json::from_str(&read("plan-blocked.json")).expect("the plan decodes")
}

/// `detail-partially-ready.json`: `erp-development`, revision 3.
pub fn detail() -> EnvironmentDetail {
    serde_json::from_str(&read("detail-partially-ready.json")).expect("the detail decodes")
}

/// `plan-locked-blocked.json`: a plan with `locked` true, a stale lock,
/// a lock without an artifact for this Mac, and an invalid local file.
pub fn locked_blocked() -> EnvPlan {
    serde_json::from_str(&read("plan-locked-blocked.json")).expect("the plan decodes")
}

/// `lock-written.json`: postgres updated from 18.5 to 18.6 and `old`
/// removed, written to `/Users/dev/erp/stackly.lock`.
pub fn lock_written() -> EnvLockResult {
    serde_json::from_str(&read("lock-written.json")).expect("the lock result decodes")
}

/// `lock-blocked.json`: one `env.lock_invalid` blocker, nothing written.
pub fn lock_blocked() -> EnvLockResult {
    serde_json::from_str(&read("lock-blocked.json")).expect("the lock result decodes")
}

/// `export-dotenv.json`: three postgres variables, the postgres socket
/// and cache skipped.
pub fn export_dotenv() -> EnvExport {
    serde_json::from_str(&read("export-dotenv.json")).expect("the export decodes")
}

/// `detail-bringing-up.json`: `graph` while its Up runs; db ready, cache
/// starting, api waiting.
pub fn bringing_up() -> EnvironmentDetail {
    serde_json::from_str(&read("detail-bringing-up.json")).expect("the detail decodes")
}

/// `detail-up-incomplete.json`: `graph` after an Up that ended with cache
/// needing a secret and api skipped.
pub fn up_incomplete() -> EnvironmentDetail {
    serde_json::from_str(&read("detail-up-incomplete.json")).expect("the detail decodes")
}

/// `graph` after an Up that brought every service up: the running report
/// of [`bringing_up`] with each service ready on its own `auto` port.
pub fn brought_up() -> EnvironmentDetail {
    use stackly_protocol::env::{EnvironmentStatus, UpPhase, UpState};
    let mut detail = bringing_up();
    let template = detail.services[0]
        .instance
        .clone()
        .expect("db has an instance");
    for (index, service) in detail.services.iter_mut().enumerate() {
        if service.instance.is_none() {
            let mut instance = template.clone();
            instance.name = format!("graph-{}", service.alias);
            instance.id =
                uuid::Uuid::from_u128(0x0192_f3a4_0000_7000_8000_0000_0000_00a0 + index as u128);
            for endpoint in &mut instance.endpoints {
                endpoint.port = Some(49152 + index as u16);
            }
            service.instance = Some(instance);
        }
        service.not_ready = None;
    }
    let report = detail.last_up.as_mut().expect("a report");
    report.state = UpState::Succeeded;
    report.finished_at = Some("2026-11-13T09:01:00.000Z".parse().expect("a fixed time"));
    for service in &mut report.services {
        service.phase = UpPhase::Ready;
        service.waiting_on.clear();
    }
    detail.summary.status = EnvironmentStatus::Ready;
    detail.summary.ready_count = Count::saturating(3);
    detail.summary.active_operation = None;
    detail
}

/// The validation of the applicable plan: valid, no findings.
pub fn valid() -> EnvValidation {
    applicable().validation
}

/// A plan of a manifest that is not valid: no items, no digest.
pub fn invalid_plan() -> EnvPlan {
    let mut plan = applicable();
    plan.validation = invalid();
    plan.applicable = false;
    plan.plan_digest = None;
    plan.items.clear();
    plan.order.clear();
    plan.detached.clear();
    plan.needs_secrets = Count::saturating(0);
    plan
}

/// `env.list` with the one fixture environment.
pub fn list() -> EnvironmentList {
    EnvironmentList {
        items: vec![detail().summary],
    }
}

/// The handler of a daemon that knows `list`, answers `validate` and
/// `plan` for every path and for a registered id, imports into
/// `imported`, and removes what `env.list` names.
pub fn serving(
    list: EnvironmentList,
    validation: EnvValidation,
    plan: EnvPlan,
    imported: EnvironmentDetail,
) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    move |call, _| match call {
        Call::EnvValidate(_) => vec![Step::Reply(Reply::EnvValidate(Box::new(
            validation.clone(),
        )))],
        Call::EnvPlan(params) => match &params.source {
            EnvSource::Registered(id) if !list.items.iter().any(|item| item.id == *id) => {
                vec![Step::Fail(error(
                    "env.not_found",
                    "not_found",
                    "no environment has this id",
                ))]
            }
            _ => vec![Step::Reply(Reply::EnvPlan(Box::new(plan.clone())))],
        },
        Call::EnvList => vec![Step::Reply(Reply::EnvList(list.clone()))],
        Call::EnvImport(_) => vec![Step::Reply(Reply::EnvImport(Box::new(imported.clone())))],
        Call::EnvRemove(params) => match list.items.iter().find(|item| item.id == params.id) {
            Some(item) if item.revision == params.expected_revision => {
                vec![Step::Reply(Reply::EnvRemove(RemovedEnvironment {
                    id: item.id,
                    name: item.name.clone(),
                }))]
            }
            Some(_) => vec![Step::Fail(error(
                "resource.revision_mismatch",
                "conflict",
                "the environment changed",
            ))],
            None => vec![Step::Fail(error(
                "env.not_found",
                "not_found",
                "no environment has this id",
            ))],
        },
        _ => Vec::new(),
    }
}

/// `detail` at `revision`.
pub fn at_revision(mut detail: EnvironmentDetail, revision: u64) -> EnvironmentDetail {
    detail.summary.revision = Revision::new(revision).expect("a small revision");
    detail
}
