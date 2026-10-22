//! The runtime DTOs the scripted daemon answers with: the wire fixtures of
//! `protocols/fixtures/runtime/`, and the operation of an install on the
//! runtime they describe.

use std::path::PathBuf;

use stackly_protocol::operation::{
    OperationAccepted, OperationKind, OperationResult, OperationState, OperationSummary,
};
use stackly_protocol::runtime::{
    InstallOutcome, InstallRuntimeReply, InstallingRuntime, ResolvedArtifact, RuntimeDetail,
    RuntimeListPage,
};
use stackly_protocol::{Call, Reply, ResourceKind, ResourceRef};
use uuid::Uuid;

use crate::common::dto;
use crate::common::scripted::{Ctx, Step, error};
use crate::play::subscribed;

/// The runtime of the detail fixture.
pub const RUNTIME: Uuid = Uuid::from_u128(0x0192_f3a6_0000_7000_8000_0000_0000_00a1);
/// The digest of its artifact.
pub const SHA256: &str = "88bb7533e639e852e5258c293aa42f0abf40aed42d18f0f714cb47a9354b6590";

fn read<T: serde::de::DeserializeOwned>(file: &str) -> T {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/runtime")
        .join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} reads: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// The artifact `runtime.resolve` names.
pub fn resolved() -> ResolvedArtifact {
    read("resolved-postgresql.json")
}

/// The runtime `runtime.get` returns.
pub fn detail() -> RuntimeDetail {
    read("detail-postgresql.json")
}

/// The page `runtime.list` returns: two runtimes and one install.
pub fn list() -> RuntimeListPage {
    read("list.json")
}

/// The list with the install of `operation` on `RUNTIME`, named PostgreSQL
/// 18.6 like the artifact the scripts resolve.
pub fn installing_list() -> RuntimeListPage {
    let mut page = list();
    page.items.clear();
    page.installing = vec![InstallingRuntime {
        operation_id: dto::OPERATION,
        runtime_id: RUNTIME,
        provider: "official/postgresql".to_owned(),
        software: "postgresql".to_owned(),
        display_name: "PostgreSQL".to_owned(),
        version: "18.6".to_owned(),
        target: resolved().target,
        sha256: SHA256.to_owned(),
    }];
    page
}

/// The install operation in `state`, on the runtime of the detail fixture;
/// a succeeded one names that runtime in its result.
pub fn operation(state: OperationState, revision: u64) -> OperationSummary {
    let mut summary = dto::summary(dto::OPERATION, state, revision);
    summary.kind = OperationKind::InstallRuntime;
    let runtime = ResourceRef {
        kind: ResourceKind::Runtime,
        id: RUNTIME.to_string(),
    };
    summary.resource = Some(runtime.clone());
    if state == OperationState::Succeeded {
        summary.result = Some(OperationResult {
            resources: vec![runtime],
            partial: Vec::new(),
        });
    }
    summary
}

/// `runtime.install` answering that `operation` started.
pub fn started(operation: OperationSummary, replayed: bool) -> Reply {
    Reply::RuntimeInstall(Box::new(InstallRuntimeReply {
        resolved: resolved(),
        outcome: InstallOutcome::Started(Box::new(OperationAccepted {
            operation,
            replayed,
        })),
    }))
}

/// `runtime.install` answering that the artifact is installed already.
pub fn already_installed() -> Reply {
    Reply::RuntimeInstall(Box::new(InstallRuntimeReply {
        resolved: resolved(),
        outcome: InstallOutcome::AlreadyInstalled(Box::new(detail().summary)),
    }))
}

/// A handler for `install`: `reply` answers it, the operation plays
/// `states`, and the reads answer the fixtures.
pub fn scripted(
    reply: Reply,
    states: Vec<OperationSummary>,
    list: RuntimeListPage,
) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    let play = crate::play::playing(states, None);
    move |call, ctx| match call {
        Call::RuntimeResolve(_) => vec![Step::Reply(Reply::RuntimeResolve(Box::new(resolved())))],
        Call::RuntimeInstall(_) => vec![Step::Reply(reply.clone())],
        Call::RuntimeGet(_) => vec![Step::Reply(Reply::RuntimeGet(Box::new(detail())))],
        Call::RuntimeList(_) => vec![Step::Reply(Reply::RuntimeList(Box::new(list.clone())))],
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        other => play(other, ctx),
    }
}

/// The error of a refusal.
pub fn refusal(code: &str, category: &str) -> Step {
    Step::Fail(error(code, category, "refused"))
}
