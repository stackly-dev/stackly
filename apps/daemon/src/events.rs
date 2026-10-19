//! The event bus (spec 0006, *Event dan cursor*).
//!
//! One mutex holds the sequence counter, a ring of the last
//! [`EVENT_REPLAY_LEN`] events, and a broadcast sender of
//! [`EVENT_BUFFER_LEN`] events. Publishing is a sync call that stamps the
//! time, takes the next sequence, appends to the ring, and broadcasts, all
//! under that lock, so the ring and the broadcast always agree and a
//! publish never waits for a subscriber. A subscription copies the ring
//! after its cursor and opens its broadcast receiver under the same lock,
//! so it misses and repeats nothing; a receiver that falls behind gets an
//! `events.gap` with reason `overflow`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_platform::Clock;
use stackly_protocol::codes::request;
use stackly_protocol::events::{Event, EventCursor, EventKind, Gap, GapReason, SubscribeParams};
use stackly_protocol::json::FieldCode;
use stackly_protocol::limits;
use stackly_protocol::{Count, ErrorBody, FieldError, ResourceRef, Revision, Sequence, Timestamp};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

/// How many events the ring keeps for replay.
pub const EVENT_REPLAY_LEN: usize = limits::EVENT_REPLAY_LEN as usize;
/// How many events a subscriber may fall behind before it gets a gap.
pub const EVENT_BUFFER_LEN: usize = limits::EVENT_BUFFER_LEN as usize;

/// One published event, before it is addressed to a subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    /// Its place in this daemon run, starting at 1.
    pub sequence: Sequence,
    /// When it was published.
    pub at: Timestamp,
    /// The resource it is about, for a resource event.
    pub resource: Option<ResourceRef>,
    /// The resource's revision after the change, for a resource event.
    pub revision: Option<Revision>,
    /// What happened.
    pub kind: EventKind,
}

impl Published {
    /// The wire event for one subscription.
    #[must_use]
    pub fn to_event(&self, subscription_id: u32) -> Event {
        Event {
            subscription_id,
            sequence: self.sequence,
            at: self.at,
            resource: self.resource.clone(),
            revision: self.revision,
            kind: self.kind.clone(),
        }
    }
}

/// The daemon's event bus, shared by every connection.
pub struct EventBus {
    server_instance_id: Uuid,
    clock: Arc<dyn Clock>,
    inner: Mutex<Inner>,
    published: AtomicU64,
    dropped: AtomicU64,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("server_instance_id", &self.server_instance_id)
            .field("head", &self.head())
            .finish_non_exhaustive()
    }
}

struct Inner {
    sequence: u64,
    ring: VecDeque<Arc<Published>>,
    sender: broadcast::Sender<Arc<Published>>,
}

impl EventBus {
    /// A bus for one daemon run, stamping events with `clock`.
    #[must_use]
    pub fn new(server_instance_id: Uuid, clock: Arc<dyn Clock>) -> Self {
        let (sender, _) = broadcast::channel(EVENT_BUFFER_LEN);
        Self {
            server_instance_id,
            clock,
            inner: Mutex::new(Inner {
                sequence: 0,
                ring: VecDeque::with_capacity(EVENT_REPLAY_LEN),
                sender,
            }),
            published: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// The server instance ID of this run.
    #[must_use]
    pub const fn server_instance_id(&self) -> Uuid {
        self.server_instance_id
    }

    /// The cursor of the last published event; sequence 0 before any.
    #[must_use]
    pub fn head(&self) -> EventCursor {
        let sequence = self.lock().sequence;
        self.cursor(sequence)
    }

    /// Publishes an event about `resource` at `revision`, the revision after
    /// the change (the last revision plus 1 for a delete). Returns its
    /// sequence.
    pub fn publish_resource(
        &self,
        kind: EventKind,
        resource: ResourceRef,
        revision: Revision,
    ) -> Sequence {
        self.publish(kind, Some(resource), Some(revision))
    }

    /// Publishes an event about the daemon itself, without a resource.
    /// Returns its sequence.
    pub fn publish_daemon(&self, kind: EventKind) -> Sequence {
        self.publish(kind, None, None)
    }

    /// How many events were published in this run.
    #[must_use]
    pub fn published(&self) -> Count {
        Count::saturating(self.published.load(Ordering::Relaxed))
    }

    /// How many events subscribers lost by falling behind, summed over
    /// every subscriber (so it can exceed [`EventBus::published`]).
    #[must_use]
    pub fn dropped(&self) -> Count {
        Count::saturating(self.dropped.load(Ordering::Relaxed))
    }

    /// Starts a subscription for `subscription_id` with `params`.
    ///
    /// Under the bus lock it opens the broadcast receiver, reads the head,
    /// and copies the ring after `params.since`. A cursor from another run
    /// starts with an `events.gap` of reason `server_restarted`, and one
    /// older than the ring with reason `cursor_expired`; both carry the
    /// head's sequence.
    ///
    /// # Errors
    ///
    /// Returns `request.invalid_params` (field `since.sequence`, code
    /// `out_of_range`) for a cursor of this run beyond the head.
    pub fn subscribe(
        self: &Arc<Self>,
        subscription_id: u32,
        params: SubscribeParams,
    ) -> Result<BusSubscription, Box<ErrorBody>> {
        let inner = self.lock();
        let head = inner.sequence;
        let mut backlog = VecDeque::new();
        match params.since {
            None => {}
            Some(since) if since.server_instance_id != self.server_instance_id => {
                backlog.push_back(self.gap(GapReason::ServerRestarted, head));
            }
            Some(since) if since.sequence.get() > head => {
                let error =
                    ErrorBody::new(&request::INVALID_PARAMS, "params do not match the method")
                        .with_detail("method", "events.subscribe")
                        .with_field(FieldError::new("since.sequence", FieldCode::OutOfRange));
                return Err(Box::new(error));
            }
            Some(since) => {
                let since = since.sequence.get();
                let oldest = inner
                    .ring
                    .front()
                    .map_or(head + 1, |event| event.sequence.get());
                if since + 1 < oldest {
                    backlog.push_back(self.gap(GapReason::CursorExpired, head));
                } else {
                    backlog.extend(
                        inner
                            .ring
                            .iter()
                            .filter(|event| event.sequence.get() > since)
                            .cloned(),
                    );
                }
            }
        }
        let receiver = inner.sender.subscribe();
        drop(inner);
        Ok(BusSubscription {
            bus: Arc::clone(self),
            subscription_id,
            params,
            head: self.cursor(head),
            backlog,
            receiver,
            seen: head,
        })
    }

    fn publish(
        &self,
        kind: EventKind,
        resource: Option<ResourceRef>,
        revision: Option<Revision>,
    ) -> Sequence {
        let mut inner = self.lock();
        inner.sequence += 1;
        let event = Arc::new(Published {
            sequence: Sequence::saturating(inner.sequence),
            at: Timestamp::from_jiff(self.clock.now()),
            resource,
            revision,
            kind,
        });
        if inner.ring.len() == EVENT_REPLAY_LEN {
            inner.ring.pop_front();
        }
        inner.ring.push_back(Arc::clone(&event));
        // No receiver is fine: nobody is subscribed yet.
        let _ = inner.sender.send(Arc::clone(&event));
        self.published.fetch_add(1, Ordering::Relaxed);
        event.sequence
    }

    fn gap(&self, reason: GapReason, sequence: u64) -> Arc<Published> {
        Arc::new(Published {
            sequence: Sequence::saturating(sequence),
            at: Timestamp::from_jiff(self.clock.now()),
            resource: None,
            revision: None,
            kind: EventKind::EventsGap(Gap { reason }),
        })
    }

    const fn cursor(&self, sequence: u64) -> EventCursor {
        EventCursor {
            server_instance_id: self.server_instance_id,
            sequence: Sequence::saturating(sequence),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One subscription's view of the bus: the replayed backlog first, then
/// new events, filtered by its params.
pub struct BusSubscription {
    bus: Arc<EventBus>,
    subscription_id: u32,
    params: SubscribeParams,
    head: EventCursor,
    backlog: VecDeque<Arc<Published>>,
    receiver: broadcast::Receiver<Arc<Published>>,
    /// The last sequence taken from the broadcast receiver.
    seen: u64,
}

impl std::fmt::Debug for BusSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BusSubscription")
            .field("subscription_id", &self.subscription_id)
            .field("head", &self.head)
            .finish_non_exhaustive()
    }
}

impl BusSubscription {
    /// The subscription ID.
    #[must_use]
    pub const fn subscription_id(&self) -> u32 {
        self.subscription_id
    }

    /// The head when the subscription started, for `Subscribed`.
    #[must_use]
    pub const fn head(&self) -> EventCursor {
        self.head
    }

    /// Waits for the next event this subscription accepts, in sequence
    /// order and without repeats. `events.gap` and `daemon.shutting_down`
    /// always pass the filter. Returns `None` once the bus is gone.
    pub async fn next(&mut self) -> Option<Event> {
        loop {
            if let Some(event) = self.backlog.pop_front() {
                let event = event.to_event(self.subscription_id);
                if self.params.accepts(&event) {
                    return Some(event);
                }
                continue;
            }
            match self.receiver.recv().await {
                Ok(published) => {
                    if published.sequence.get() <= self.seen {
                        continue;
                    }
                    self.seen = published.sequence.get();
                    let event = published.to_event(self.subscription_id);
                    if self.params.accepts(&event) {
                        return Some(event);
                    }
                }
                Err(RecvError::Lagged(lost)) => {
                    self.bus.dropped.fetch_add(lost, Ordering::Relaxed);
                    self.seen = self.seen.saturating_add(lost);
                    let gap = self.bus.gap(GapReason::Overflow, self.seen);
                    return Some(gap.to_event(self.subscription_id));
                }
                Err(RecvError::Closed) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use stackly_platform::SystemClock;
    use stackly_protocol::events::{ShutdownReason, ShuttingDown};
    use stackly_protocol::{ResourceKind, codes};
    use tokio::time::timeout;

    use super::*;

    fn bus() -> Arc<EventBus> {
        Arc::new(EventBus::new(Uuid::now_v7(), Arc::new(SystemClock)))
    }

    fn shutting_down() -> EventKind {
        EventKind::DaemonShuttingDown(ShuttingDown {
            reason: ShutdownReason::Signal,
        })
    }

    fn since(bus: &EventBus, sequence: u64) -> SubscribeParams {
        SubscribeParams {
            since: Some(EventCursor {
                server_instance_id: bus.server_instance_id(),
                sequence: Sequence::saturating(sequence),
            }),
            ..SubscribeParams::default()
        }
    }

    async fn take(subscription: &mut BusSubscription, count: usize) -> Vec<Event> {
        let mut events = Vec::with_capacity(count);
        for _ in 0..count {
            let next = timeout(Duration::from_secs(2), subscription.next())
                .await
                .expect("an event arrives in time")
                .expect("the bus is still there");
            events.push(next);
        }
        events
    }

    fn sequences(events: &[Event]) -> Vec<u64> {
        events.iter().map(|event| event.sequence.get()).collect()
    }

    fn gap_reason(event: &Event) -> Option<GapReason> {
        match &event.kind {
            EventKind::EventsGap(gap) => Some(gap.reason),
            _ => None,
        }
    }

    #[test]
    fn sequences_start_at_one_and_rise_by_one_per_event() {
        let bus = bus();
        assert_eq!(bus.head().sequence, Sequence::ZERO);

        let first = bus.publish_daemon(shutting_down());
        let second = bus.publish_daemon(shutting_down());

        assert_eq!((first.get(), second.get()), (1, 2));
        assert_eq!(bus.head().sequence.get(), 2);
        assert_eq!(bus.head().server_instance_id, bus.server_instance_id());
        assert_eq!(bus.published().get(), 2);
    }

    #[tokio::test]
    async fn a_resource_event_carries_its_resource_and_revision_and_a_daemon_event_neither() {
        let bus = bus();
        let resource = ResourceRef {
            kind: ResourceKind::Instance,
            id: Uuid::now_v7().to_string(),
        };
        let mut subscription = bus.subscribe(1, SubscribeParams::default()).unwrap();

        bus.publish_resource(shutting_down(), resource.clone(), Revision::saturating(7));
        bus.publish_daemon(shutting_down());

        let events = take(&mut subscription, 2).await;
        assert_eq!(events[0].resource.as_ref(), Some(&resource));
        assert_eq!(events[0].revision, Some(Revision::saturating(7)));
        assert_eq!(
            (events[1].resource.as_ref(), events[1].revision),
            (None, None)
        );
        assert!(events.iter().all(|event| event.subscription_id == 1));
    }

    #[tokio::test]
    async fn a_new_subscription_starts_at_the_head_and_misses_nothing_after_it() {
        let bus = bus();
        bus.publish_daemon(shutting_down());
        let mut subscription = bus.subscribe(3, SubscribeParams::default()).unwrap();
        assert_eq!(subscription.head().sequence.get(), 1);

        for _ in 0..5 {
            bus.publish_daemon(shutting_down());
        }

        assert_eq!(
            sequences(&take(&mut subscription, 5).await),
            [2, 3, 4, 5, 6]
        );
    }

    #[tokio::test]
    async fn replay_after_a_cursor_has_no_repeats_and_no_holes_then_continues_live() {
        let bus = bus();
        for _ in 0..10 {
            bus.publish_daemon(shutting_down());
        }
        let mut subscription = bus.subscribe(1, since(&bus, 4)).unwrap();
        for _ in 0..3 {
            bus.publish_daemon(shutting_down());
        }

        let events = take(&mut subscription, 9).await;

        assert_eq!(sequences(&events), (5..=13).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn a_cursor_at_the_head_replays_nothing() {
        let bus = bus();
        bus.publish_daemon(shutting_down());
        let mut subscription = bus.subscribe(1, since(&bus, 1)).unwrap();
        bus.publish_daemon(shutting_down());

        assert_eq!(sequences(&take(&mut subscription, 1).await), [2]);
    }

    #[tokio::test]
    async fn a_cursor_older_than_the_ring_starts_with_cursor_expired_after_5000_events() {
        let bus = bus();
        for _ in 0..5000 {
            bus.publish_daemon(shutting_down());
        }
        let mut subscription = bus.subscribe(1, since(&bus, 1)).unwrap();
        bus.publish_daemon(shutting_down());

        let events = take(&mut subscription, 2).await;

        assert_eq!(gap_reason(&events[0]), Some(GapReason::CursorExpired));
        assert_eq!(events[0].sequence.get(), 5000);
        assert_eq!(events[1].sequence.get(), 5001);
    }

    #[tokio::test]
    async fn the_oldest_cursor_the_ring_still_covers_replays_in_full() {
        let bus = bus();
        for _ in 0..5000 {
            bus.publish_daemon(shutting_down());
        }
        // The ring holds 905..=5000, so a cursor at 904 misses nothing.
        let oldest = 5000 - EVENT_REPLAY_LEN as u64;
        let mut subscription = bus.subscribe(1, since(&bus, oldest)).unwrap();

        let events = take(&mut subscription, EVENT_REPLAY_LEN).await;

        assert_eq!(events[0].sequence.get(), oldest + 1);
        assert!(events.iter().all(|event| gap_reason(event).is_none()));
    }

    #[tokio::test]
    async fn a_cursor_of_another_run_starts_with_server_restarted() {
        let bus = bus();
        bus.publish_daemon(shutting_down());
        let params = SubscribeParams {
            since: Some(EventCursor {
                server_instance_id: Uuid::now_v7(),
                sequence: Sequence::saturating(1),
            }),
            ..SubscribeParams::default()
        };
        let mut subscription = bus.subscribe(1, params).unwrap();
        bus.publish_daemon(shutting_down());

        let events = take(&mut subscription, 2).await;

        assert_eq!(gap_reason(&events[0]), Some(GapReason::ServerRestarted));
        assert_eq!(events[0].sequence.get(), 1);
        assert_eq!(events[1].sequence.get(), 2);
    }

    #[test]
    fn a_cursor_of_this_run_beyond_the_head_is_invalid() {
        let bus = bus();
        bus.publish_daemon(shutting_down());

        let error = bus.subscribe(1, since(&bus, 2)).unwrap_err();

        assert!(error.is(&codes::request::INVALID_PARAMS));
        let field = &error.fields()[0];
        assert_eq!(
            (field.path(), field.code()),
            ("since.sequence", "out_of_range")
        );
    }

    #[tokio::test]
    async fn a_subscriber_that_falls_behind_gets_an_overflow_gap_and_publish_never_waits() {
        let bus = bus();
        let mut subscription = bus.subscribe(1, SubscribeParams::default()).unwrap();

        let started = std::time::Instant::now();
        for _ in 0..(EVENT_BUFFER_LEN + 500) {
            bus.publish_daemon(shutting_down());
        }
        assert!(started.elapsed() < Duration::from_secs(2));

        let events = take(&mut subscription, 2).await;

        assert_eq!(gap_reason(&events[0]), Some(GapReason::Overflow));
        assert_eq!(events[0].sequence.get(), 500, "the last lost sequence");
        assert_eq!(events[1].sequence.get(), 501);
        assert_eq!(bus.dropped().get(), 500);
    }

    #[tokio::test]
    async fn gaps_and_shutdown_pass_any_filter() {
        let bus = bus();
        let params = SubscribeParams {
            kinds: vec!["operation.".to_owned()],
            resource: Some(ResourceRef {
                kind: ResourceKind::Operation,
                id: Uuid::now_v7().to_string(),
            }),
            since: Some(EventCursor {
                server_instance_id: Uuid::now_v7(),
                sequence: Sequence::ZERO,
            }),
        };
        let mut subscription = bus.subscribe(1, params).unwrap();
        bus.publish_daemon(shutting_down());

        let events = take(&mut subscription, 2).await;

        assert_eq!(gap_reason(&events[0]), Some(GapReason::ServerRestarted));
        assert_eq!(events[1].kind, shutting_down());
    }
}
