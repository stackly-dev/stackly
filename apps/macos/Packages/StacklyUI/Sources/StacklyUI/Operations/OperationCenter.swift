import Foundation
import Observation
import StacklyClient

/// An operation as the shell keeps it: the summary DTO of spec 0007.
public typealias OperationSnapshot = OperationSummary

/// The operations that have not ended, kept from snapshots plus `operation.` events
/// (spec 0013, *Operation center*).
///
/// Its store loads `operation.list` with `activeOnly` and follows `operation.state_changed`
/// and `operation.progress`. The running and cancelling operations give the Operations
/// badge and subtitle. An operation that reaches a final state leaves ``active`` and stays
/// in ``recent`` for 5 minutes, so a screen that listens late through ``updates(for:)``
/// still gets its end.
@Observable
public final class OperationCenter {
    /// How long an ended operation stays in ``recent``, in seconds.
    static var recentRetention: TimeInterval { 300 }

    /// The store the shell keeps in sync.
    public let store: ResourceStore<OperationSnapshot>

    /// The ended operations by ID, with when the center saw them end.
    private var ended: [UUID: Ended] = [:]
    @ObservationIgnored private var watchers:
        [UUID: [UUID: AsyncStream<OperationSnapshot>.Continuation]] = [:]
    /// The revision last sent to the watchers of each operation.
    @ObservationIgnored private var published: [UUID: UInt64] = [:]
    @ObservationIgnored private var lookingUp: Set<UUID> = []
    @ObservationIgnored private let fetchOne: (UUID) async throws(ClientError) -> OperationSnapshot?
    @ObservationIgnored private let now: () -> Date

    /// An operation in a final state.
    private struct Ended {
        let operation: OperationSnapshot
        let at: Date
    }

    /// Creates an empty center.
    ///
    /// - Parameters:
    ///   - fetchPage: Fetches one page of the operations that have not ended.
    ///   - fetchOne: Fetches one operation, or `nil` when the daemon no longer knows it.
    ///   - now: The wall clock.
    ///   - sleep: Waits; tests inject one that does not.
    public init(
        fetchPage: @escaping (PageToken?) async throws(ClientError) -> Page<OperationSnapshot>,
        fetchOne: @escaping (UUID) async throws(ClientError) -> OperationSnapshot?,
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        self.fetchOne = fetchOne
        self.now = now
        store = ResourceStore(
            kinds: ["operation."],
            fetchPage: fetchPage,
            fetchOne: fetchOne,
            revision: { $0.revision },
            reduce: Self.reduce,
            now: now,
            sleep: sleep)
        store.didApply = { [weak self] change in self?.storeDidApply(change) }
    }

    /// A center that reads through `call`: `operation.list` with `activeOnly` for
    /// snapshots and `operation.get` for one operation.
    ///
    /// - Parameters:
    ///   - call: Sends one call to the daemon.
    ///   - now: The wall clock.
    public convenience init(
        call: @escaping (Call) async throws(ClientError) -> Reply,
        now: @escaping () -> Date = { Date() }
    ) {
        self.init(
            fetchPage: { token throws(ClientError) in try await Self.page(after: token, call: call)
            },
            fetchOne: { id throws(ClientError) in try await Self.operation(id, call: call) },
            now: now)
    }

    // MARK: - What the shell shows

    /// The operations that have not ended (queued, running, cancelling, or needing
    /// recovery), newest first like `operation.list`.
    public var active: [OperationSnapshot] {
        store.items.values.filter { Self.isActive($0.state) }.sorted(by: Self.newestFirst)
    }

    /// The operations that are running or cancelling, or `nil` before the first snapshot
    /// loaded, so no badge or subtitle shows a number the daemon never sent.
    public var runningCount: Int? {
        guard store.loadedAt != nil else { return nil }
        return store.items.values.count { $0.state == .running || $0.state == .cancelRequested }
    }

    /// The Operations page subtitle, or `nil` before the first snapshot loaded.
    public var subtitle: String? {
        Self.subtitle(running: runningCount)
    }

    /// The operations that ended in the last 5 minutes, newest first.
    public var recent: [OperationSnapshot] {
        let cutoff = now().addingTimeInterval(-Self.recentRetention)
        return ended.values.filter { $0.at > cutoff }.map(\.operation).sorted(by: Self.newestFirst)
    }

    /// The Operations page subtitle for a running count: "{n} running", "No running
    /// operations", or `nil` when the count never loaded.
    public static func subtitle(running: Int?) -> String? {
        guard let running else { return nil }
        return running == 0 ? "No running operations" : "\(running) running"
    }

    // MARK: - Following one operation

    /// The operation's current value, then every change, ending after its final state.
    ///
    /// An operation the center does not hold is fetched with `operation.get`. The stream
    /// ends without a value when the daemon no longer knows the operation.
    public func updates(for id: UUID) -> AsyncStream<OperationSnapshot> {
        let (stream, continuation) = AsyncStream.makeStream(of: OperationSnapshot.self)
        pruneEnded()
        if let ended = ended[id] {
            continuation.yield(ended.operation)
            continuation.finish()
            return stream
        }
        let token = UUID()
        watchers[id, default: [:]][token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { @MainActor in self?.stopWatching(id, token: token) }
        }
        if let current = store.items[id] {
            continuation.yield(current)
        } else {
            lookUp(id)
        }
        return stream
    }

    private func stopWatching(_ id: UUID, token: UUID) {
        watchers[id]?[token] = nil
        if watchers[id]?.isEmpty == true {
            watchers[id] = nil
            published[id] = nil
        }
    }

    // MARK: - Store changes

    private func storeDidApply(_ change: StoreChange<OperationSnapshot>) {
        switch change {
        case .changed(let operation):
            if Self.isActive(operation.state) {
                publish(operation)
            } else {
                end(operation)
            }
        case .removed(let id):
            close(id)
        case .reloaded:
            for operation in store.items.values where Self.isActive(operation.state) == false {
                end(operation)
            }
            // A watched operation missing from the snapshot ended while the shell was
            // away; `operation.get` tells how.
            for id in watchers.keys {
                if let operation = store.items[id] {
                    publish(operation)
                } else {
                    lookUp(id)
                }
            }
        }
    }

    /// Sends a newer value to the operation's watchers.
    private func publish(_ operation: OperationSnapshot) {
        guard let continuations = watchers[operation.id] else { return }
        if let sent = published[operation.id], sent >= operation.revision { return }
        published[operation.id] = operation.revision
        for continuation in continuations.values {
            continuation.yield(operation)
        }
    }

    /// Moves an ended operation from the store to ``recent`` and ends its watchers.
    private func end(_ operation: OperationSnapshot) {
        let isNewer = ended[operation.id].map { $0.operation.revision < operation.revision } ?? true
        if isNewer {
            ended[operation.id] = Ended(operation: operation, at: now())
        }
        pruneEnded()
        store.evict(operation.id)
        let continuations = watchers.removeValue(forKey: operation.id) ?? [:]
        published[operation.id] = nil
        for continuation in continuations.values {
            continuation.yield(operation)
            continuation.finish()
        }
    }

    /// Ends the watchers of an operation the daemon no longer knows.
    private func close(_ id: UUID) {
        let continuations = watchers.removeValue(forKey: id) ?? [:]
        published[id] = nil
        for continuation in continuations.values {
            continuation.finish()
        }
    }

    /// Fetches a watched operation the store does not hold. Without a connection it
    /// waits for the next snapshot, which looks again.
    private func lookUp(_ id: UUID) {
        guard lookingUp.insert(id).inserted else { return }
        Task {
            defer { lookingUp.remove(id) }
            let fetched: OperationSnapshot?
            do {
                fetched = try await fetchOne(id)
            } catch {
                return
            }
            guard watchers[id] != nil else { return }
            guard let fetched else {
                close(id)
                return
            }
            if Self.isActive(fetched.state) {
                store.offer(fetched)
            } else {
                end(fetched)
            }
        }
    }

    private func pruneEnded() {
        let cutoff = now().addingTimeInterval(-Self.recentRetention)
        ended = ended.filter { $0.value.at > cutoff }
    }

    // MARK: - Rules

    /// Whether an operation in `state` has not ended.
    static func isActive(_ state: OperationState) -> Bool {
        switch state {
        case .queued, .running, .cancelRequested, .needsRecovery: true
        case .succeeded, .failed, .cancelled: false
        }
    }

    /// Newest first, by creation time then ID, the order of `operation.list`.
    private static func newestFirst(_ a: OperationSnapshot, _ b: OperationSnapshot) -> Bool {
        if a.createdAt != b.createdAt { return a.createdAt > b.createdAt }
        return a.id.uuidString > b.id.uuidString
    }

    /// Both operation events carry the whole summary at its committed revision.
    static func reduce(_ event: Event) -> StoreEffect<OperationSnapshot> {
        switch event.kind {
        case .operationStateChanged(let operation), .operationProgress(let operation):
            .upsert(operation)
        default:
            .ignore
        }
    }

    // MARK: - Calls

    /// One page of `operation.list` with `activeOnly`, after `token`.
    static func page(
        after token: PageToken?,
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> Page<OperationSnapshot> {
        let params = ListOperationsParams(
            activeOnly: true, resource: nil, limit: nil, before: token?.value)
        guard case .operationList(let page) = try await call(.operationList(params)) else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return Page(items: page.items ?? [], next: page.next.map(PageToken.init))
    }

    /// One operation from `operation.get`, or `nil` when the daemon answers
    /// `resource.not_found`.
    static func operation(
        _ id: UUID,
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> OperationSnapshot? {
        let reply: Reply
        do throws(ClientError) {
            reply = try await call(.operationGet(GetOperationParams(operationId: id)))
        } catch {
            if case .rejected(let body) = error, body.code == "resource.not_found" { return nil }
            throw error
        }
        guard case .operationGet(let detail) = reply else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return detail.operation
    }
}
