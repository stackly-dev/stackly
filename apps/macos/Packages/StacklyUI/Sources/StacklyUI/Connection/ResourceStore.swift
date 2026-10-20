import Foundation
import Observation
import StacklyClient

/// A page token from the owner of a list method.
public struct PageToken: Hashable, Sendable {
    /// The token's text, as the daemon sent it.
    public let value: String

    /// Wraps a token.
    public init(_ value: String) {
        self.value = value
    }
}

/// One page of a list method.
public struct Page<Item: Sendable>: Sendable {
    /// The items of this page.
    public let items: [Item]
    /// The token of the next page, or `nil` on the last page.
    public let next: PageToken?

    /// Creates a page.
    public init(items: [Item], next: PageToken?) {
        self.items = items
        self.next = next
    }
}

/// What one event does to a store.
public enum StoreEffect<Item: Identifiable & Sendable> {
    /// The item, whole, at its revision.
    case upsert(Item)
    /// A change to a known item at `revision`; an unknown item is fetched.
    case patch(Item.ID, revision: UInt64, (inout Item) -> Void)
    /// The item is gone at `revision`.
    case delete(Item.ID, revision: UInt64)
    /// The item changed at `revision` in a way only a fetch shows.
    case refetch(Item.ID, revision: UInt64)
    /// A status observation without a revision (spec 0010, `statusSeq`). For a held
    /// item, `apply` runs and says whether the item changed; the held revision and the
    /// tombstones never move. For an item the store does not hold, it is a `refetch` at
    /// `revision`, so a deleted item is not brought back.
    case observe(Item.ID, revision: UInt64, apply: (inout Item) -> Bool)
    /// The event does not concern this store.
    case ignore
}

/// A change a store applied, for an owner that derives more from the items.
enum StoreChange<Item: Identifiable> {
    /// The item was added or changed, after the revision rule let it in.
    case changed(Item)
    /// The item was removed.
    case removed(Item.ID)
    /// A snapshot replaced the items.
    case reloaded
}

/// The last known state of one kind of resource, kept by snapshots plus
/// events (spec 0013, *Sinkronisasi snapshot plus cursor*).
///
/// A change applies only when its revision is above the held one or the
/// tombstone's; an older snapshot never overwrites a newer event; a snapshot
/// of an older generation is dropped; events that arrive during a snapshot
/// wait and apply after it, in order.
@Observable
public final class ResourceStore<Item: Identifiable & Sendable> where Item.ID: Sendable {
    /// The most events kept while a snapshot runs.
    static var bufferLimit: Int { 4_096 }
    /// The waits before a snapshot starts over after its buffer filled.
    static var overflowDelays: [Duration] { [.seconds(1), .seconds(2), .seconds(4)] }
    /// The wait before a snapshot is tried again while the daemon starts.
    static var startingDelay: Duration { .seconds(1) }

    /// The items by ID.
    public private(set) var items: [Item.ID: Item] = [:]
    /// When the last snapshot completed, or `nil` before the first.
    public private(set) var loadedAt: Date?
    /// The error of the last snapshot, kept next to the old data.
    public private(set) var error: ErrorBody?
    /// Whether a snapshot runs.
    public private(set) var isRefreshing = false
    /// Whether the daemon does not know the list method (an older daemon).
    public private(set) var unsupported = false

    /// The revision held for each item.
    private(set) var revisions: [Item.ID: UInt64] = [:]
    /// The revision of each deleted item, until the next snapshot completes.
    private(set) var tombstones: [Item.ID: UInt64] = [:]
    /// The snapshot generation this store works on.
    private(set) var generation = 0
    /// Events held while a snapshot runs.
    @ObservationIgnored private var buffer: [Event]?
    @ObservationIgnored private var bufferOverflowed = false
    /// Called after every change the store applied, so an owner such as
    /// ``OperationCenter`` can follow the items.
    @ObservationIgnored var didApply: ((StoreChange<Item>) -> Void)?

    @ObservationIgnored let kinds: [String]
    @ObservationIgnored private let fetchPage: (PageToken?) async throws(ClientError) -> Page<Item>
    @ObservationIgnored private let fetchOne: (Item.ID) async throws(ClientError) -> Item?
    @ObservationIgnored private let revision: (Item) -> UInt64
    @ObservationIgnored private let reduce: (Event) -> StoreEffect<Item>
    @ObservationIgnored private let merge: (_ held: Item, _ incoming: Item) -> Item
    @ObservationIgnored private let isGone: (Item) -> Bool
    @ObservationIgnored private let resetObservations: ((inout Item) -> Void)?
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private let sleep: (Duration) async throws -> Void

    /// Creates an empty store.
    ///
    /// - Parameters:
    ///   - kinds: The event kinds it follows: full kinds, or area prefixes
    ///     ending in a dot such as `instance.`.
    ///   - fetchPage: Fetches one page of the list method.
    ///   - fetchOne: Fetches one item, or `nil` when it is gone.
    ///   - revision: The revision of an item.
    ///   - reduce: What an event does to the store.
    ///   - merge: Joins a whole item that arrives (an upsert, a refetch, an offer, or a
    ///     held event applied after a snapshot) with the held one. The default takes the
    ///     incoming item. A snapshot replaces items as they are.
    ///   - isGone: Whether an item that arrives is a removal in disguise, such as a
    ///     retained instance. Such an item never enters the store: it deletes the held
    ///     one at its revision.
    ///   - resetObservations: Forgets the observation bound of a held item, called for
    ///     every item when the daemon restarted (`server_restarted`), so the counters of
    ///     the new run are not taken for old ones.
    ///   - now: The wall clock for ``loadedAt``.
    ///   - sleep: Waits; tests inject one that does not.
    public init(
        kinds: [String],
        fetchPage: @escaping (PageToken?) async throws(ClientError) -> Page<Item>,
        fetchOne: @escaping (Item.ID) async throws(ClientError) -> Item?,
        revision: @escaping (Item) -> UInt64,
        reduce: @escaping (Event) -> StoreEffect<Item>,
        merge: @escaping (_ held: Item, _ incoming: Item) -> Item = { _, incoming in incoming },
        isGone: @escaping (Item) -> Bool = { _ in false },
        resetObservations: ((inout Item) -> Void)? = nil,
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        self.kinds = kinds
        self.fetchPage = fetchPage
        self.fetchOne = fetchOne
        self.revision = revision
        self.reduce = reduce
        self.merge = merge
        self.isGone = isGone
        self.resetObservations = resetObservations
        self.now = now
        self.sleep = sleep
    }

    /// The number of items.
    public var count: Int {
        items.count
    }

    /// Whether the store follows events of `kind`.
    public func follows(_ kind: String) -> Bool {
        kinds.contains { kind == $0 || ($0.hasSuffix(".") && kind.hasPrefix($0)) }
    }

    // MARK: - Snapshots

    /// Loads a full snapshot in a new generation: every page, then the events
    /// held meanwhile. A snapshot started later wins, and the result of an
    /// older generation is dropped. After a full buffer it starts over, at
    /// most 3 times with waits of 1, 2, and 4 seconds; while the daemon starts
    /// it tries again every second.
    ///
    /// - Returns: Whether this generation completed or failed for good, so the
    ///   shell knows the store is settled; `false` when a newer generation
    ///   took over.
    @discardableResult
    func resnapshot() async -> Bool {
        generation += 1
        let generation = self.generation
        var overflows = 0
        while true {
            guard generation == self.generation else { return false }
            begin()
            let result = await fetchAll()
            guard generation == self.generation else { return false }
            switch result {
            case .success(let snapshot) where bufferOverflowed:
                _ = snapshot
                guard overflows < Self.overflowDelays.count else {
                    fail(Self.overflowError)
                    return true
                }
                try? await sleep(Self.overflowDelays[overflows])
                overflows += 1
            case .success(let snapshot):
                complete(snapshot)
                return true
            case .failure(let failure):
                switch failure.body.code {
                case "daemon.starting":
                    try? await sleep(Self.startingDelay)
                case "request.unknown_method":
                    unsupported = true
                    fail(nil)
                    return true
                default:
                    fail(failure.body)
                    return true
                }
            }
        }
    }

    /// Loads a snapshot for this store only, as the page's Retry does.
    public func refresh() async {
        await resnapshot()
    }

    /// Starts holding events for a snapshot.
    private func begin() {
        buffer = []
        bufferOverflowed = false
        isRefreshing = true
    }

    private func fetchAll() async -> Result<[Item], ClientError> {
        var all: [Item] = []
        var token: PageToken?
        repeat {
            do {
                let page = try await fetchPage(token)
                all.append(contentsOf: page.items)
                token = page.next
            } catch {
                return .failure(error)
            }
        } while token != nil
        return .success(all)
    }

    /// Replaces the items with `snapshot`, keeps deletes newer than it, then
    /// applies the held events in order.
    private func complete(_ snapshot: [Item]) {
        var fresh: [Item.ID: Item] = [:]
        var freshRevisions: [Item.ID: UInt64] = [:]
        for item in snapshot where isGone(item) == false {
            let itemRevision = revision(item)
            if let tombstone = tombstones[item.id], tombstone >= itemRevision { continue }
            fresh[item.id] = item
            freshRevisions[item.id] = itemRevision
        }
        items = fresh
        revisions = freshRevisions
        let held = buffer ?? []
        buffer = nil
        for event in held {
            apply(event)
        }
        tombstones.removeAll()
        loadedAt = now()
        error = nil
        isRefreshing = false
        didApply?(.reloaded)
    }

    /// Ends a snapshot that failed: the old data stays, the held events
    /// apply, and `body` is kept as the error.
    private func fail(_ body: ErrorBody?) {
        let held = buffer ?? []
        buffer = nil
        for event in held {
            apply(event)
        }
        error = body
        isRefreshing = false
    }

    // MARK: - Events

    /// Takes one event: held while a snapshot runs, else applied now.
    func receive(_ event: Event) {
        guard var held = buffer else {
            apply(event)
            return
        }
        guard held.count < Self.bufferLimit else {
            bufferOverflowed = true
            return
        }
        held.append(event)
        buffer = held
    }

    private func apply(_ event: Event) {
        switch reduce(event) {
        case .upsert(let item):
            upsert(item)
        case .patch(let id, let newRevision, let change):
            guard var item = items[id] else {
                refetch(id, revision: newRevision)
                return
            }
            guard newRevision > floor(of: id) else { return }
            change(&item)
            items[id] = item
            revisions[id] = newRevision
            didApply?(.changed(item))
        case .delete(let id, let newRevision):
            guard newRevision > floor(of: id) else { return }
            remove(id, revision: newRevision)
        case .refetch(let id, let newRevision):
            refetch(id, revision: newRevision)
        case .observe(let id, let newRevision, let change):
            guard var item = items[id] else {
                refetch(id, revision: newRevision)
                return
            }
            // Neither the revision nor a tombstone moves: an observation is not a
            // change of the row, so a state change at the same revision still applies.
            if change(&item) {
                items[id] = item
                didApply?(.changed(item))
            }
        case .ignore:
            break
        }
    }

    private func upsert(_ incoming: Item) {
        let itemRevision = revision(incoming)
        guard itemRevision > floor(of: incoming.id) else { return }
        if isGone(incoming) {
            remove(incoming.id, revision: itemRevision)
            return
        }
        let item = items[incoming.id].map { merge($0, incoming) } ?? incoming
        items[item.id] = item
        revisions[item.id] = itemRevision
        tombstones[item.id] = nil
        didApply?(.changed(item))
    }

    /// Tells the store the daemon restarted: items forget their observation bound, so the
    /// first observation of the new run applies even when its counter is below the old
    /// one. A snapshot follows; this matters when that snapshot fails and the old data
    /// stays.
    func serverRestarted() {
        guard let resetObservations else { return }
        items = items.mapValues { held in
            var item = held
            resetObservations(&item)
            return item
        }
    }

    /// Removes an item at `revision` and keeps the revision as a tombstone, so
    /// an older change cannot bring it back.
    private func remove(_ id: Item.ID, revision newRevision: UInt64) {
        items[id] = nil
        revisions[id] = nil
        tombstones[id] = newRevision
        didApply?(.removed(id))
    }

    /// Applies an item fetched outside a snapshot, under the revision rule.
    func offer(_ item: Item) {
        upsert(item)
    }

    /// Drops a held item without telling the owner, keeping its revision as a
    /// tombstone. ``OperationCenter`` moves an ended operation out this way.
    func evict(_ id: Item.ID) {
        guard let held = revisions[id] else { return }
        items[id] = nil
        revisions[id] = nil
        tombstones[id] = held
    }

    /// The revision a change must be above: the held one or the tombstone's.
    private func floor(of id: Item.ID) -> UInt64 {
        max(revisions[id] ?? 0, tombstones[id] ?? 0)
    }

    private func refetch(_ id: Item.ID, revision newRevision: UInt64) {
        guard newRevision > floor(of: id) else { return }
        Task {
            let fetched: Item?
            do {
                fetched = try await fetchOne(id)
            } catch {
                // The next snapshot brings the item; nothing changes now.
                return
            }
            guard let fetched else {
                // Gone: the change it announced was a delete.
                if newRevision > floor(of: id) {
                    remove(id, revision: newRevision)
                }
                return
            }
            upsert(fetched)
        }
    }

    /// The error a store keeps when its snapshot could not catch up with the
    /// events three times in a row.
    static var overflowError: ErrorBody {
        ErrorBody(
            code: "internal.unexpected", category: .internal,
            message: "the client could not keep up with the daemon's events", retryable: false,
            resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
            items: nil)
    }
}
