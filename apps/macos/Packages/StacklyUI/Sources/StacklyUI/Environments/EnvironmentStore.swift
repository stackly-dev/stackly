import Foundation
import Observation
import StacklyClient

/// The registered environments, for the sidebar, the Environments page, and the Overview
/// (spec 0044, *Layar* and *Event*).
/// Its store loads `env.list` in one page, since the daemon answers at most 64 environments
/// by name. There is no environment event kind: the store loads `env.list` again when an
/// `operation.state_changed` that ended carries a resource of kind `environment`, and, 300 ms
/// after the last one, when `instance.state_changed`, `instance.health_changed`, or
/// `instance.removed` arrives, since an instance changes the ready count. A reconnect loads it
/// through the shell's resnapshot. Every reload also raises ``changes``, so an open detail page
/// reads `env.get` again with `checkManifest` false and never reads the user's file on an
/// instance event.
@MainActor
@Observable
public final class EnvironmentStore {
    /// How long instance events wait before the store loads again.
    static let instanceDelay: Duration = .milliseconds(300)

    /// The store the shell keeps in sync.
    public let store: ResourceStore<EnvironmentSummary>
    /// Counts the loads that followed an event, so an open detail page reads its detail again.
    public private(set) var changes = 0
    /// The last environment operation that ended, so the detail page of that environment reads
    /// its manifest again.
    public private(set) var lastEnded: EndedOperation?
    /// Sends one read to the daemon.
    @ObservationIgnored let call: (Call) async throws(ClientError) -> Reply
    /// Sends one mutation with its idempotency key through the mutation path.
    @ObservationIgnored let send: (Call, String) async -> MutationOutcome
    @ObservationIgnored private let refresher: SnapshotCoalescer
    @ObservationIgnored private let sleep: (Duration) async throws -> Void
    @ObservationIgnored private var pendingInstanceReload: Task<Void, Never>?

    /// Creates an empty store.
    /// - Parameters:
    ///   - call: Sends one read to the daemon (`env.list`, `env.get`, `env.validate`,
    ///     `env.plan`).
    ///   - send: Sends one mutation with its idempotency key (`env.import`, `env.remove`); by
    ///     default nothing is sent.
    ///   - now: The wall clock.
    ///   - sleep: Waits; tests inject one that does not.
    public init(
        call: @escaping (Call) async throws(ClientError) -> Reply,
        send: @escaping (Call, String) async -> MutationOutcome = { _, _ in
            .notSent(ClientErrorCatalog.notConnected())
        },
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        self.call = call
        self.send = send
        self.sleep = sleep
        let box = EventBox()
        let store = ResourceStore<EnvironmentSummary>(
            kinds: [
                "operation.state_changed", "instance.state_changed", "instance.health_changed",
                "instance.removed",
            ],
            fetchPage: { _ throws(ClientError) in
                Page(items: try await Self.list(call: call), next: nil)
            },
            fetchOne: { id throws(ClientError) in
                try await Self.list(call: call).first { $0.id == id }
            },
            revision: { $0.revision },
            reduce: { event in
                box.run?(event)
                return .ignore
            },
            now: now,
            sleep: sleep)
        let refresher = SnapshotCoalescer { [weak store] in _ = await store?.resnapshot() }
        self.store = store
        self.refresher = refresher
        box.run = { [weak self] event in self?.receive(event) }
        store.didApply = { [weak self] change in
            if case .reloaded = change { self?.changes += 1 }
        }
    }

    /// The action an event runs, set once the store exists.
    private final class EventBox {
        var run: (@MainActor (Event) -> Void)?
    }

    /// What one event does: an ended environment operation reloads now, an instance event
    /// reloads after ``instanceDelay``, and anything else changes nothing.
    func receive(_ event: Event) {
        switch event.kind {
        case .operationStateChanged(let operation):
            guard operation.resource?.kind == .environment, Self.hasEnded(operation.state)
            else { return }
            pendingInstanceReload?.cancel()
            pendingInstanceReload = nil
            lastEnded = EndedOperation(
                operation: operation.id,
                environment: operation.resource.flatMap { UUID(uuidString: $0.id) })
            refresher.request()
        case .instanceStateChanged, .instanceHealthChanged, .instanceRemoved:
            pendingInstanceReload?.cancel()
            let sleep = sleep
            pendingInstanceReload = Task { [weak self] in
                do {
                    try await sleep(Self.instanceDelay)
                } catch {
                    return
                }
                guard Task.isCancelled == false else { return }
                self?.pendingInstanceReload = nil
                self?.refresher.request()
            }
        default:
            return
        }
    }

    /// Whether an operation in `state` ended.
    static func hasEnded(_ state: OperationState) -> Bool {
        switch state {
        case .succeeded, .failed, .cancelled, .needsRecovery: true
        case .queued, .running, .cancelRequested: false
        }
    }

    /// Loads `env.list` again, such as after an import or a remove, without waiting for the
    /// operation event. It shares a snapshot with the events.
    func reload() async {
        await refresher.run()
    }

    /// The environments by name, or `nil` before the first snapshot.
    public var environments: [EnvironmentSummary]? {
        guard store.loadedAt != nil else { return nil }
        return store.items.values.sorted {
            $0.name.localizedStandardCompare($1.name) == .orderedAscending
        }
    }

    /// The environment with `id`, when the last snapshot holds it.
    public func environment(_ id: UUID) -> EnvironmentSummary? {
        store.items[id]
    }

    /// The Environments subtitle: "1 project" or "{n} projects", or `nil` before the first
    /// snapshot.
    public var subtitle: String? {
        environments.map {
            SidebarSection.countText($0.count, singular: "project", plural: "projects")
        }
    }

    // MARK: - Calls

    /// `env.list`.
    static func list(
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> [EnvironmentSummary] {
        guard case .envList(let reply) = try await call(.envList) else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return reply.items ?? []
    }

    /// `env.get`, reading the manifest file only when `checkManifest` is true.
    func detail(_ id: UUID, checkManifest: Bool) async throws(ClientError) -> EnvironmentDetail {
        guard
            case .envGet(let detail) = try await call(
                .envGet(EnvGetParams(id: id, checkManifest: checkManifest)))
        else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return detail
    }

    /// `env.validate` for the file at `path`.
    func validate(path: String) async throws(ClientError) -> EnvValidation {
        let reply = try await call(.envValidate(EnvValidateParams(path: path)))
        guard case .envValidate(let validation) = reply else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return validation
    }

    /// `env.plan` for `source`.
    func plan(_ source: EnvSource) async throws(ClientError) -> EnvPlan {
        guard
            case .envPlan(let plan) = try await call(
                .envPlan(EnvPlanParams(source: source, locked: nil)))
        else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return plan
    }
}

/// An environment operation that ended (spec 0044, *Event*).
public struct EndedOperation: Equatable, Sendable {
    /// The operation.
    public let operation: UUID
    /// The environment it worked on, when its resource ID reads as one.
    public let environment: UUID?
}
