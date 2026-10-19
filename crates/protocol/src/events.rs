//! Events, cursors, and the `events.subscribe` and `events.unsubscribe`
//! methods (AC-9).
//!
//! A cursor is `(serverInstanceId, sequence)` and lives in daemon memory for
//! one run. Clients follow the snapshot plus cursor recipe of spec 0006:
//! subscribe first, then take the snapshot, then dedupe by sequence and by
//! resource revision.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::error::FieldError;
use crate::json::{FieldCode, Validate};
use crate::operation::OperationSummary;
use crate::wire::{ResourceRef, Revision, Sequence, Timestamp};

/// Most entries in `SubscribeParams.kinds`.
pub const SUBSCRIBE_KINDS_MAX: usize = 32;

/// One event as a daemon frame carries it.
///
/// An event about a resource always carries both `resource` and `revision`;
/// a daemon event carries neither.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// The subscription this event is delivered for.
    pub subscription_id: u32,
    /// Position in this daemon run, rising by one per published event.
    pub sequence: Sequence,
    /// When the event was published.
    pub at: Timestamp,
    /// The resource the event is about.
    #[serde(default)]
    pub resource: Option<ResourceRef>,
    /// The resource's revision after the change.
    #[serde(default)]
    pub revision: Option<Revision>,
    /// What happened.
    pub kind: EventKind,
}

/// What happened. Each kind is `<area>.<name>`; the owner of an area adds
/// its kinds here. Clients skip kinds they do not know through
/// [`EventProbe`], so a new kind is a minor bump.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "content")]
pub enum EventKind {
    /// The subscriber missed events; take a new snapshot.
    #[serde(rename = "events.gap")]
    EventsGap(Gap),
    /// The daemon is shutting down.
    #[serde(rename = "daemon.shutting_down")]
    DaemonShuttingDown(ShuttingDown),
    /// An operation was created or changed state (spec 0007). The event's
    /// resource is the operation and its revision the operation's.
    #[serde(rename = "operation.state_changed")]
    OperationStateChanged(Box<OperationSummary>),
    /// An operation changed without changing state: stage, progress,
    /// `waitingOn`, the point of no return, or a cancel (spec 0007).
    #[serde(rename = "operation.progress")]
    OperationProgress(Box<OperationSummary>),
}

/// Lists every event kind once, giving `EventKind::KINDS` and
/// `EventKind::name` from the same entries. A kind missing here does not
/// compile.
macro_rules! event_kinds {
    ($($name:literal => $variant:ident;)*) => {
        impl EventKind {
            /// Every event kind this build knows.
            pub const KINDS: &[&str] = &[$($name),*];

            /// The wire name of this kind, such as `events.gap`.
            #[must_use]
            pub const fn name(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => $name,)*
                }
            }
        }
    };
}

event_kinds! {
    "events.gap" => EventsGap;
    "daemon.shutting_down" => DaemonShuttingDown;
    "operation.state_changed" => OperationStateChanged;
    "operation.progress" => OperationProgress;
}

impl EventKind {
    /// Whether every subscriber receives this kind whatever its filter:
    /// gaps and `daemon.shutting_down`.
    #[must_use]
    pub const fn always_delivered(&self) -> bool {
        matches!(self, Self::EventsGap(_) | Self::DaemonShuttingDown(_))
    }
}

/// Content of `events.gap`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Gap {
    /// Why events were missed.
    pub reason: GapReason,
}

/// Why a subscriber missed events. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GapReason {
    /// The subscriber fell more than the event buffer behind.
    Overflow,
    /// The cursor is older than the replay ring.
    CursorExpired,
    /// The cursor belongs to another daemon run.
    ServerRestarted,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// Content of `daemon.shutting_down`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShuttingDown {
    /// Why the daemon stops.
    pub reason: ShutdownReason,
}

/// Why the daemon stops. Open: newer daemons may add values.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownReason {
    /// SIGTERM or SIGINT.
    Signal,
    /// A client asked it to stop.
    Requested,
    /// A reason this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// A position in the event stream of one daemon run.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventCursor {
    /// The daemon run the sequence belongs to.
    pub server_instance_id: Uuid,
    /// The last sequence seen; 0 means no event yet.
    pub sequence: Sequence,
}

/// Params of `events.subscribe`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeParams {
    /// Replay every event after this cursor, or null for new events only.
    #[serde(default)]
    pub since: Option<EventCursor>,
    /// Full kinds such as `instance.state_changed` or area prefixes ending
    /// in a dot such as `operation.`; empty means every kind. At most 32.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// Only events about this resource.
    #[serde(default)]
    pub resource: Option<ResourceRef>,
}

impl SubscribeParams {
    /// Whether this subscription receives `event`: gaps and
    /// `daemon.shutting_down` always, others when they match both `kinds`
    /// and `resource`.
    #[must_use]
    pub fn accepts(&self, event: &Event) -> bool {
        event.kind.always_delivered()
            || (self.matches_kind(event.kind.name())
                && self.matches_resource(event.resource.as_ref()))
    }

    /// Whether the event kind `name` matches `kinds`: empty matches every
    /// kind, an entry ending in a dot matches its area.
    #[must_use]
    pub fn matches_kind(&self, name: &str) -> bool {
        self.kinds.is_empty()
            || self.kinds.iter().any(|filter| {
                filter == name || (filter.ends_with('.') && name.starts_with(filter.as_str()))
            })
    }

    /// Whether an event about `resource` matches the `resource` filter.
    #[must_use]
    pub fn matches_resource(&self, resource: Option<&ResourceRef>) -> bool {
        self.resource
            .as_ref()
            .is_none_or(|wanted| resource == Some(wanted))
    }
}

impl Validate for SubscribeParams {
    /// `kinds` holds at most [`SUBSCRIBE_KINDS_MAX`] entries, and each is a
    /// kind filter. Only the first [`SUBSCRIBE_KINDS_MAX`] entries are
    /// checked, so a list far over the limit costs no more than a full one.
    fn validate(&self) -> Vec<FieldError> {
        let mut fields = Vec::new();
        if self.kinds.len() > SUBSCRIBE_KINDS_MAX {
            fields.push(FieldError::new("kinds", FieldCode::TooMany));
        }
        for (index, kind) in self.kinds.iter().enumerate().take(SUBSCRIBE_KINDS_MAX) {
            if !is_kind_filter(kind) {
                fields.push(FieldError::new(
                    format!("kinds[{index}]"),
                    FieldCode::InvalidFormat,
                ));
            }
        }
        fields
    }
}

/// Whether `text` matches `^[a-z][a-z0-9_]*\.([a-z][a-z0-9_]*)?$`: a full
/// kind, or an area prefix ending in a dot.
#[must_use]
pub fn is_kind_filter(text: &str) -> bool {
    let Some((area, name)) = text.split_once('.') else {
        return false;
    };
    is_snake(area) && (name.is_empty() || is_snake(name))
}

/// `^[a-z][a-z0-9_]*$`.
fn is_snake(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Reply of `events.subscribe`. It always precedes the subscription's first
/// event.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Subscribed {
    /// Starts at 1 per connection and is never reused on it.
    pub subscription_id: u32,
    /// The newest cursor when the subscription started.
    pub head: EventCursor,
}

/// Params of `events.unsubscribe`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeParams {
    /// The subscription to end.
    pub subscription_id: u32,
}

impl Validate for UnsubscribeParams {}

/// The part of an event frame a client reads first, so it can skip a kind
/// it does not know without failing the stream. The skipped sequence still
/// advances the cursor.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventProbe {
    /// The subscription the event is delivered for.
    pub subscription_id: u32,
    /// The event's sequence.
    pub sequence: Sequence,
    /// The event kind's tag only.
    pub kind: EventKindProbe,
}

/// The tag of an event kind, without its content.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EventKindProbe {
    /// The event kind, such as `events.gap`.
    #[serde(rename = "type")]
    pub r#type: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::ResourceKind;

    fn event(kind: EventKind, resource: Option<ResourceRef>) -> Event {
        Event {
            subscription_id: 1,
            sequence: Sequence::new(7).unwrap(),
            at: Timestamp::UNIX_EPOCH,
            revision: resource.as_ref().map(|_| Revision::new(3).unwrap()),
            resource,
            kind,
        }
    }

    fn shutdown() -> EventKind {
        EventKind::DaemonShuttingDown(ShuttingDown {
            reason: ShutdownReason::Signal,
        })
    }

    fn instance(id: &str) -> ResourceRef {
        ResourceRef {
            kind: ResourceKind::Instance,
            id: id.to_owned(),
        }
    }

    #[test]
    fn every_known_kind_has_a_variant_with_the_same_serde_tag() {
        for kind in EventKind::KINDS {
            let err = serde_json::from_value::<EventKind>(serde_json::json!({ "type": kind }))
                .unwrap_err()
                .to_string();
            assert!(!err.contains("unknown variant"), "{kind}: {err}");
        }
        assert_eq!(shutdown().name(), "daemon.shutting_down");
        let json = serde_json::to_value(shutdown()).unwrap();
        assert_eq!(json["type"], shutdown().name());
    }

    #[test]
    fn gaps_and_shutdown_pass_every_filter() {
        let params = SubscribeParams {
            kinds: vec!["instance.state_changed".to_owned()],
            resource: Some(instance("a")),
            ..SubscribeParams::default()
        };
        assert!(params.accepts(&event(shutdown(), None)));
        let gap = EventKind::EventsGap(Gap {
            reason: GapReason::Overflow,
        });
        assert!(params.accepts(&event(gap, None)));
    }

    #[test]
    fn empty_kinds_match_everything_and_prefixes_match_an_area() {
        assert!(SubscribeParams::default().matches_kind("instance.state_changed"));
        let params = SubscribeParams {
            kinds: vec!["operation.".to_owned(), "instance.state_changed".to_owned()],
            ..SubscribeParams::default()
        };
        assert!(params.matches_kind("operation.progress"));
        assert!(params.matches_kind("instance.state_changed"));
        assert!(!params.matches_kind("instance.health_changed"));
        assert!(!params.matches_kind("operations.progress"));
    }

    #[test]
    fn a_resource_filter_matches_only_events_about_that_resource() {
        assert!(SubscribeParams::default().matches_resource(None));
        let params = SubscribeParams {
            resource: Some(instance("a")),
            ..SubscribeParams::default()
        };
        assert!(params.matches_resource(Some(&instance("a"))));
        assert!(!params.matches_resource(Some(&instance("b"))));
        assert!(!params.matches_resource(None));
    }

    #[test]
    fn kind_filters_must_be_a_full_kind_or_an_area_prefix() {
        assert!(is_kind_filter("instance.state_changed"));
        assert!(is_kind_filter("operation."));
        assert!(!is_kind_filter("operation"));
        assert!(!is_kind_filter(".gap"));
        assert!(!is_kind_filter("Instance.state"));
        assert!(!is_kind_filter("a.b.c"));
    }

    #[test]
    fn subscribe_validation_flags_each_bad_kind_and_too_many_kinds() {
        let params = SubscribeParams {
            kinds: vec!["ok.kind".to_owned(), "BAD".to_owned()],
            ..SubscribeParams::default()
        };
        let fields = params.validate();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            (fields[0].path(), fields[0].code()),
            ("kinds[1]", "invalid_format")
        );

        let many = SubscribeParams {
            kinds: vec!["events.".to_owned(); SUBSCRIBE_KINDS_MAX + 1],
            ..SubscribeParams::default()
        };
        let fields = many.validate();
        assert_eq!((fields[0].path(), fields[0].code()), ("kinds", "too_many"));
    }

    #[test]
    fn foreign_gap_and_shutdown_reasons_decode_as_unrecognized() {
        let gap: GapReason = serde_json::from_str("\"compacted\"").unwrap();
        assert_eq!(gap, GapReason::Unrecognized);
        let reason: ShutdownReason = serde_json::from_str("\"upgrade\"").unwrap();
        assert_eq!(reason, ShutdownReason::Unrecognized);
    }
}
