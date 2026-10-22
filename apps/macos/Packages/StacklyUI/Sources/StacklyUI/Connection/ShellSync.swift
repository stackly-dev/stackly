import Foundation
import Observation
import StacklyClient

/// A store the shell keeps in sync, whatever its item type.
protocol SyncedStore: AnyObject {
    /// Whether the store follows events of `kind`.
    func follows(_ kind: String) -> Bool
    /// Takes one event.
    func receive(_ event: Event)
    /// Loads a snapshot in a new generation; `true` once it settled.
    func resnapshot() async -> Bool
    /// The daemon restarted: forget observation bounds before the snapshot starts.
    func serverRestarted()
    /// Whether the last snapshot failed with an error and none runs now, so a
    /// reconnect that caught up loads one again.
    var snapshotFailed: Bool { get }
}

extension ResourceStore: SyncedStore {
    var snapshotFailed: Bool {
        error != nil && isRefreshing == false
    }
}

/// A view that follows the events of one kind prefix without owning a store.
private struct EventObserver {
    let prefix: String
    let continuation: AsyncStream<Event>.Continuation
}

/// Applies the sync engine's signals to the shell's stores, in one ordered
/// loop on the main actor (spec 0013, *Sinkronisasi snapshot plus cursor*).
///
/// A resnapshot signal starts a new generation in every store; each store
/// loads all pages and drops a result of an older generation. Events go to
/// the stores that follow their kind. A reconnect that caught up loads a
/// snapshot again only in the stores whose last one failed, once per
/// `.caughtUp`; an `unsupported` store waits for the next resnapshot, which a
/// new daemon run always brings.
@Observable
public final class ShellSync {
    @ObservationIgnored private var stores: [any SyncedStore] = []
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var pending: Set<ObjectIdentifier> = []
    @ObservationIgnored private var loop: Task<Void, Never>?
    @ObservationIgnored private var observers: [UUID: EventObserver] = [:]

    /// Whether no snapshot runs: every store settled the latest generation.
    public private(set) var isSettled = true
    /// Counts the times the stores became current: after every completed
    /// snapshot round, and after a reconnect that caught up without one.
    public private(set) var completedRounds = 0
    /// When the last frame arrived on a subscription.
    public private(set) var lastFrameAt: Date?
    /// Why the stores last loaded a full snapshot, or `nil` before the first.
    public private(set) var lastResnapshot: ResnapshotReason?
    /// Called after a snapshot round settled, so the daemon model can update.
    @ObservationIgnored var onChange: (() -> Void)?

    /// Creates the shell sync with no store.
    public init() {}

    /// Adds a store. A store added after a snapshot started loads its own
    /// snapshot now.
    public func register<Item>(_ store: ResourceStore<Item>) {
        stores.append(store)
        if generation > 0 {
            snapshot(store, generation: generation)
        }
    }

    /// The events whose kind starts with `kindPrefix` (spec 0013, *ShellSync*, added by spec
    /// 0021): for a view that follows a kind without a store of its own, such as the detail
    /// page for `secret.`.
    ///
    /// Each call returns a stream of its own. It yields the events that arrive after the call
    /// and stops being fed when the stream is dropped or `ShellSync` is stopped. It does not
    /// replay anything a snapshot or a reconnect missed; the observer reads its own state
    /// again after a resnapshot.
    ///
    /// - Parameter kindPrefix: The start of the event kind, such as `secret.`.
    /// - Returns: The stream of events of that kind.
    public func events(kindPrefix: String) -> AsyncStream<Event> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: Event.self, bufferingPolicy: .bufferingNewest(64))
        let id = UUID()
        observers[id] = EventObserver(prefix: kindPrefix, continuation: continuation)
        continuation.onTermination = { [weak self] _ in
            Task { @MainActor in self?.observers[id] = nil }
        }
        return stream
    }

    /// How many views follow a kind prefix now, for tests.
    var observerCount: Int { observers.count }

    /// Starts applying `signals`, in order, until they end.
    public func start(_ signals: AsyncStream<SyncSignal>) {
        loop?.cancel()
        loop = Task {
            for await signal in signals {
                self.handle(signal)
            }
        }
    }

    /// Stops applying signals.
    public func stop() {
        loop?.cancel()
        loop = nil
        for observer in observers.values {
            observer.continuation.finish()
        }
        observers.removeAll()
    }

    /// Applies one signal.
    func handle(_ signal: SyncSignal) {
        switch signal {
        case .resnapshot(let reason):
            lastResnapshot = reason
            if case .gap(.serverRestarted) = reason {
                // Counters such as `statusSeq` start again at 1 in a new run (spec 0010).
                for store in stores {
                    store.serverRestarted()
                }
            }
            generation += 1
            pending = Set(stores.map(ObjectIdentifier.init))
            isSettled = pending.isEmpty
            if isSettled {
                completedRounds += 1
            }
            for store in stores {
                snapshot(store, generation: generation)
            }
        case .event(let event):
            let kind = event.kind.kindName
            for store in stores where store.follows(kind) {
                store.receive(event)
            }
            for observer in observers.values where kind.hasPrefix(observer.prefix) {
                observer.continuation.yield(event)
            }
        case .caughtUp:
            let failed = stores.filter(\.snapshotFailed)
            guard failed.isEmpty else {
                // The round ends when these snapshots settle, worked or not.
                for store in failed {
                    snapshot(store, generation: generation)
                }
                return
            }
            if isSettled {
                completedRounds += 1
            }
        case .lastFrameAt(let date):
            lastFrameAt = date
        }
    }

    private func snapshot(_ store: any SyncedStore, generation: Int) {
        let id = ObjectIdentifier(store)
        pending.insert(id)
        isSettled = false
        Task {
            let settled = await store.resnapshot()
            guard settled, generation == self.generation else { return }
            pending.remove(id)
            if pending.isEmpty, isSettled == false {
                isSettled = true
                completedRounds += 1
                onChange?()
            }
        }
    }
}
