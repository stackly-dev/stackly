//! The environment DTOs the scripted daemon answers with: the wire
//! fixtures of `protocols/fixtures/env/`, and the shapes they lack made
//! from the same DTOs.

use std::path::PathBuf;

use stackly_protocol::env::{
    EnvPlan, EnvSource, EnvValidation, EnvironmentDetail, EnvironmentList, RemovedEnvironment,
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
