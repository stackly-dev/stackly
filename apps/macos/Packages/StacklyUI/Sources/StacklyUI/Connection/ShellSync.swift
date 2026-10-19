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
}

extension ResourceStore: SyncedStore {}

/// Applies the sync engine's signals to the shell's stores, in one ordered
/// loop on the main actor (spec 0013, *Sinkronisasi snapshot plus cursor*).
///
/// A resnapshot signal starts a new generation in every store; each store
/// loads all pages and drops a result of an older generation. Events go to
/// the stores that follow their kind.
@Observable
public final class ShellSync {
    @ObservationIgnored private var stores: [any SyncedStore] = []
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var pending: Set<ObjectIdentifier> = []
    @ObservationIgnored private var loop: Task<Void, Never>?

    /// Whether no snapshot runs: every store settled the latest generation.
    public private(set) var isSettled = true
    /// Counts the times the stores became current: after every completed
    /// snapshot round, and after a reconnect that caught up without one.
    public private(set) var completedRounds = 0
    /// When the last frame arrived on a subscription.
    public private(set) var lastFrameAt: Date?
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
    }

    /// Applies one signal.
    func handle(_ signal: SyncSignal) {
        switch signal {
        case .resnapshot:
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
        case .caughtUp:
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
