//! Scripted daemon handlers that play operation states.

use std::sync::Arc;
use std::time::Duration;

use stackly_protocol::events::{EventCursor, EventKind, Subscribed};
use stackly_protocol::operation::{CancelDisposition, CancelReply, OperationSummary};
use stackly_protocol::{Call, Reply, Sequence};

use crate::common::dto;
use crate::common::scripted::{Ctx, Step, server_instance};

/// The answer to `events.subscribe` on connection `index`.
pub fn subscribed(index: usize) -> Step {
    Step::Reply(Reply::EventsSubscribe(Subscribed {
        subscription_id: 1,
        head: EventCursor {
            server_instance_id: server_instance(index),
            sequence: Sequence::ZERO,
        },
    }))
}

/// The answer to `operation.get` with `summary`, then, unless it settled,
/// an event that asks for the next refresh.
pub fn got(summary: &OperationSummary) -> Vec<Step> {
    let mut steps = vec![Step::Reply(Reply::OperationGet(Box::new(dto::detail(
        summary.clone(),
    ))))];
    if !summary.state.is_final()
        && summary.state != stackly_protocol::operation::OperationState::NeedsRecovery
    {
        steps.push(Step::Sleep(Duration::from_millis(30)));
        steps.push(Step::Event(EventKind::OperationStateChanged(Box::new(
            summary.clone(),
        ))));
    }
    steps
}

/// A handler whose gets answer `states` one after another, the last one
/// for every later get, and whose `operation.cancel` answers `cancel`.
pub fn playing(
    states: Vec<OperationSummary>,
    cancel: Option<CancelDisposition>,
) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    let states = Arc::new(states);
    move |call, ctx| match call {
        Call::EventsSubscribe(_) => vec![subscribed(ctx.connection)],
        Call::OperationGet(_) => got(&states[ctx.nth.min(states.len() - 1)]),
        Call::OperationCancel(_) => {
            vec![Step::Reply(Reply::OperationCancel(Box::new(CancelReply {
                operation: states[0].clone(),
                disposition: cancel.unwrap_or(CancelDisposition::Requested),
            })))]
        }
        _ => Vec::new(),
    }
}
