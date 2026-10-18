import Foundation

/// Why the stores must load a full snapshot.
public enum ResnapshotReason: Equatable, Sendable {
    /// There was no cursor yet, so the subscription starts at the head.
    case noCursor
    /// The daemon reported a gap: events were missed.
    case gap(GapReason)
}

/// What the sync engine tells the stores.
public enum SyncSignal: Equatable, Sendable {
    /// Every store loads a full snapshot; events after this signal apply on
    /// top of it.
    case resnapshot(ResnapshotReason)
    /// An event to apply, in order, never one already applied.
    case event(Event)
    /// The subscription of a new connection caught up without a snapshot:
    /// every missed event was replayed up to the head of the handshake.
    case caughtUp
    /// A frame arrived at this time; sent at most once per second.
    case lastFrameAt(Date)
}

/// Follows the daemon's events with the snapshot plus cursor recipe of
/// spec 0006 (spec 0013, *Sinkronisasi snapshot plus cursor*).
///
/// On every new connection it subscribes to every kind from the stored cursor,
/// or from the head when there is none and then asks for a snapshot. A gap
/// asks for a snapshot again without ending the stream. When a reconnect
/// needs no snapshot, ``SyncSignal/caughtUp`` says the replay reached the
/// head, so the shell knows its state is current again. The cursor is the
/// connection's `serverInstanceId` and the last sequence received; events,
/// skipped kinds, and gaps advance it, and an event at or below it is
/// dropped. The cursor stays in memory for as long as the app runs.
public actor SyncEngine {
    private let link: DaemonLink
    private let now: @Sendable () -> Date
    private let continuation: AsyncStream<SyncSignal>.Continuation
    private var watcher: Task<Void, Never>?
    private var follower: Task<Void, Never>?
    /// The `connectedAt` of the connection being followed.
    private var following: Date?
    private var lastFrameSignal: Date?

    /// The signals for the stores, in order.
    public nonisolated let signals: AsyncStream<SyncSignal>

    /// The last event position received.
    public private(set) var cursor: EventCursor?

    /// Creates an engine for `link`; call ``start()`` to follow it.
    ///
    /// - Parameters:
    ///   - link: The link whose connections to follow.
    ///   - now: The wall clock for ``SyncSignal/lastFrameAt(_:)``.
    public init(link: DaemonLink, now: @escaping @Sendable () -> Date = { Date() }) {
        self.link = link
        self.now = now
        (signals, continuation) = AsyncStream.makeStream(of: SyncSignal.self)
    }

    /// Starts following the link's connections.
    public func start() {
        guard watcher == nil else { return }
        let states = link.states
        watcher = Task {
            for await state in states {
                self.observe(state)
            }
        }
    }

    /// Stops following and ends ``signals``.
    public func stop() {
        watcher?.cancel()
        follower?.cancel()
        continuation.finish()
    }

    private func observe(_ state: ConnectionState) {
        guard case .connected(let daemon) = state else {
            follower?.cancel()
            follower = nil
            following = nil
            return
        }
        guard following != daemon.connectedAt else { return }
        following = daemon.connectedAt
        follower?.cancel()
        follower = Task { await self.follow() }
    }

    private func follow() async {
        let params = SubscribeParams(since: cursor, kinds: [], resource: nil)
        guard let (subscribed, stream, accepted) = try? await link.subscribe(params) else { return }
        let instance = accepted.serverInstanceId
        let head = subscribed.head.sequence
        var caughtUp = false
        if let cursor {
            if cursor.serverInstanceId == instance, cursor.sequence >= head {
                caughtUp = true
                continuation.yield(.caughtUp)
            }
        } else {
            cursor = EventCursor(serverInstanceId: instance, sequence: head)
            caughtUp = true
            continuation.yield(.resnapshot(.noCursor))
        }
        do {
            for try await frame in stream {
                guard Task.isCancelled == false else { return }
                noteFrame()
                let isGap = Self.isGap(frame)
                await handle(frame, instance: instance)
                guard caughtUp == false else { continue }
                if isGap {
                    caughtUp = true
                } else if let cursor, cursor.serverInstanceId == instance, cursor.sequence >= head {
                    caughtUp = true
                    continuation.yield(.caughtUp)
                }
            }
        } catch {
            // The connection ended; the link reports why.
        }
    }

    private func handle(_ frame: DecodedFrame, instance: UUID) async {
        switch frame {
        case .skipped(_, let sequence, _):
            advance(to: sequence, instance: instance)
        case .event(let event):
            if case .eventsGap(let gap) = event.kind {
                // A gap replaces the cursor's sequence, even a lower one after a
                // restart.
                cursor = EventCursor(serverInstanceId: instance, sequence: event.sequence)
                continuation.yield(.resnapshot(.gap(gap.reason)))
            } else if case .daemonShuttingDown = event.kind {
                advance(to: event.sequence, instance: instance)
                await link.daemonShuttingDown()
            } else if advance(to: event.sequence, instance: instance) {
                continuation.yield(.event(event))
            }
        case .response, .error:
            break
        }
    }

    private static func isGap(_ frame: DecodedFrame) -> Bool {
        if case .event(let event) = frame, case .eventsGap = event.kind { return true }
        return false
    }

    /// Moves the cursor to `sequence`. Returns `false` when the event was
    /// already applied.
    @discardableResult
    private func advance(to sequence: UInt64, instance: UUID) -> Bool {
        if let cursor, cursor.serverInstanceId == instance, sequence <= cursor.sequence {
            return false
        }
        cursor = EventCursor(serverInstanceId: instance, sequence: sequence)
        return true
    }

    private func noteFrame() {
        let time = now()
        if let last = lastFrameSignal, time.timeIntervalSince(last) < 1 { return }
        lastFrameSignal = time
        continuation.yield(.lastFrameAt(time))
    }
}
