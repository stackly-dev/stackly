//! The instance DTOs the scripted daemon answers with: the wire fixtures of
//! spec 0010, and the shapes the fixtures lack made from the same DTOs.

use std::path::PathBuf;

use serde_json::Value;
use stackly_protocol::instance::{InstanceActionAccepted, InstanceSummary};
use stackly_protocol::operation::{OperationAccepted, OperationKind, OperationState};
use stackly_protocol::{Call, Reply, ResourceKind, ResourceRef};
use uuid::Uuid;

use crate::common::dto;
use crate::common::scripted::{Ctx, Step, error};
use crate::play::subscribed;

/// The instance of the `frame-response-instance-get*` fixtures.
pub const INSTANCE: Uuid = Uuid::from_u128(0x0192_f3a5_0000_7000_8000_0000_0000_0001);
/// The runtime those instances run on.
pub const RUNTIME: &str = "0192f3a5-0000-7000-8000-0000000000a1";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures/ipc")
}

/// The reply content of the fixture frame `name`.
pub fn fixture(name: &str) -> Value {
    let path = fixture_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} reads: {error}", path.display()));
    let frame: Value = serde_json::from_str(&text).expect("a fixture is JSON");
    frame["content"]["reply"]["content"].clone()
}

/// The instance of `frame-response-instance-get-<kind>.json`, or the plain
/// running one for an empty `kind`.
pub fn instance(kind: &str) -> InstanceSummary {
    let name = if kind.is_empty() {
        "frame-response-instance-get.json".to_owned()
    } else {
        format!("frame-response-instance-get-{kind}.json")
    };
    serde_json::from_value(fixture(&name)).expect("a fixture instance decodes")
}

/// An instance made from the running fixture by `edit`.
pub fn edited(edit: impl FnOnce(&mut Value)) -> InstanceSummary {
    let mut value = fixture("frame-response-instance-get.json");
    edit(&mut value);
    serde_json::from_value(value).expect("an edited instance decodes")
}

/// A `running` instance Stackly found again after a restart, in the
/// `adopted` state, that cannot be adopted a second time.
pub fn adopted() -> InstanceSummary {
    edited(|value| {
        value["process"]["adopted"] = true.into();
        value["process"]["adoptable"] = false.into();
        value["restart"]["nextAttemptAt"] = "2026-10-20T09:05:00.000Z".into();
    })
}

/// An instance `stopped` with the `needsSecret` blocker.
pub fn locked() -> InstanceSummary {
    let mut instance = instance("needs-secret");
    instance.lifecycle = stackly_protocol::instance::InstanceLifecycle::Stopped;
    instance
}

/// The operation `kind` on the fixture instance in `state`.
pub fn operation(
    kind: OperationKind,
    state: OperationState,
    revision: u64,
) -> stackly_protocol::operation::OperationSummary {
    let mut summary = dto::summary(dto::OPERATION, state, revision);
    summary.kind = kind;
    summary.resource = Some(ResourceRef {
        kind: ResourceKind::Instance,
        id: INSTANCE.to_string(),
    });
    summary
}

/// The reply to `instance.create`, `start`, or `stop` with `operation`.
pub fn accepted(
    operation: stackly_protocol::operation::OperationSummary,
    replayed: bool,
) -> InstanceActionAccepted {
    InstanceActionAccepted {
        accepted: Some(OperationAccepted {
            operation,
            replayed,
        }),
        already_in_state: false,
    }
}

/// The reply to `start` or `stop` for an instance already in the state.
pub fn already() -> InstanceActionAccepted {
    InstanceActionAccepted {
        accepted: None,
        already_in_state: true,
    }
}

/// A handler for one starting method: `reply` answers it; the operation
/// plays `states` one `operation.get` after another; `instance` answers
/// `instance.get`, or with `None` fails it `resource.not_found`.
pub fn scripted(
    reply: Reply,
    states: Vec<stackly_protocol::operation::OperationSummary>,
    instance: Option<InstanceSummary>,
) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    let play = crate::play::playing(states, None);
    move |call, ctx| match call {
        Call::InstanceCreate(_) | Call::InstanceStart(_) | Call::InstanceStop(_) => {
            vec![Step::Reply(reply.clone())]
        }
        Call::InstanceGet(_) => match &instance {
            Some(instance) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
            None => vec![Step::Fail(error(
                "resource.not_found",
                "not_found",
                "no instance has this id",
            ))],
        },
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        other => play(other, ctx),
    }
}
