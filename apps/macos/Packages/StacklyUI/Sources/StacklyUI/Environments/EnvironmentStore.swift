import Foundation
import Observation
import StacklyClient

/// The registered environments, for the sidebar, the Environments page, and the Overview
/// (spec 0044, *Layar* and *Event*).
/// Its store loads `env.list` in one page, since the daemon answers at most 64 environments
/// by name. There is no environment event kind: the store loads `env.list` again at once when
/// an `operation.state_changed` that ended carries a resource of kind `environment`, and, 300 ms
/// after the last one, on `operation.state_changed` and `operation.progress` of an environment
/// operation that has not ended, on `operation.state_changed` of an operation whose `parent` is
/// the `activeOperation` of an environment (spec 0046, *Event*), and on `instance.state_changed`,
/// `instance.health_changed`, or `instance.removed`, since an instance changes the ready count.
/// A reconnect loads it through the shell's resnapshot. Every reload also raises ``changes``, so an open detail page
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
    @ObservationIgnored private var pendingReload: Task<Void, Never>?

    /// Creates an empty store.
    /// - Parameters:
    ///   - call: Sends one read to the daemon (`env.list`, `env.get`, `env.validate`,
    ///     `env.plan`, `env.export`, `env.preview_down`, `operation.get`), and `env.lock`,
    ///     which writes only the lock file.
    ///   - send: Sends one mutation with its idempotency key (`env.import`, `env.remove`,
    ///     `env.up`, `env.down`); by default nothing is sent.
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
                "operation.state_changed", "operation.progress", "instance.state_changed",
                "instance.health_changed", "instance.removed",
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

    /// What one event does: an ended environment operation reloads now; a running one, its
    /// progress, a change of one of its child operations, and an instance event reload after
    /// ``instanceDelay``, coalesced; anything else changes nothing.
    func receive(_ event: Event) {
        switch event.kind {
        case .operationStateChanged(let operation):
            if operation.resource?.kind == .environment {
                if Self.hasEnded(operation.state) {
                    reloadNow(after: operation)
                } else {
                    scheduleReload()
                }
            } else if let parent = operation.parent, activeOperations.contains(parent) {
                scheduleReload()
            }
        case .operationProgress(let operation):
            guard operation.resource?.kind == .environment else { return }
            scheduleReload()
        case .instanceStateChanged, .instanceHealthChanged, .instanceRemoved:
            scheduleReload()
        default:
            return
        }
    }

    /// The `activeOperation` IDs of the environments of the last snapshot.
    var activeOperations: Set<UUID> {
        Set(store.items.values.compactMap { $0.activeOperation?.id })
    }

    /// Loads now for an environment operation that ended, so its page reads the manifest.
    private func reloadNow(after operation: OperationSummary) {
        pendingReload?.cancel()
        pendingReload = nil
        lastEnded = EndedOperation(
            operation: operation.id,
            environment: operation.resource.flatMap { UUID(uuidString: $0.id) })
        refresher.request()
    }

    /// Loads ``instanceDelay`` after the last of a burst of events.
    private func scheduleReload() {
        pendingReload?.cancel()
        let sleep = sleep
        pendingReload = Task { [weak self] in
            do {
                try await sleep(Self.instanceDelay)
            } catch {
                return
            }
            guard Task.isCancelled == false else { return }
            self?.pendingReload = nil
            self?.refresher.request()
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

    /// The names of the environments of the last snapshot by ID, for operation titles.
    public var names: [UUID: String] {
        store.items.mapValues(\.name)
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

    /// `env.lock` for the registered manifest of `id` (spec 0045). It writes only the lock file
    /// next to the manifest and is no operation, so it carries no idempotency key; the page
    /// gates it like a mutation.
    func lock(_ id: UUID, refresh: LockRefresh) async throws(ClientError) -> EnvLockResult {
        guard
            case .envLock(let result) = try await call(
                .envLock(EnvLockParams(source: .registered(id), refresh: refresh)))
        else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return result
    }

    /// `env.preview_down` for `id` (spec 0047, AC-1): what Down would stop and keep now. It
    /// only reads.
    func previewDown(_ id: UUID) async throws(ClientError) -> EnvDownPreview {
        guard
            case .envPreviewDown(let preview) = try await call(
                .envPreviewDown(EnvPreviewDownParams(id: id)))
        else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return preview
    }

    /// `operation.get` for `id`: the summary of an operation that ended, for the feedback of
    /// a Down (spec 0047, AC-21).
    func operation(_ id: UUID) async throws(ClientError) -> OperationSummary {
        guard
            case .operationGet(let detail) = try await call(
                .operationGet(GetOperationParams(operationId: id)))
        else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return detail.operation
    }

    /// `env.export` for the registered environment `id` in `format` (spec 0045). It only
    /// reads the registry.
    func export(_ id: UUID, format: ExportFormat) async throws(ClientError) -> EnvExport {
        guard
            case .envExport(let export) = try await call(
                .envExport(EnvExportParams(source: .registered(id), format: format)))
        else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return export
    }
}

/// An environment operation that ended (spec 0044, *Event*).
public struct EndedOperation: Equatable, Sendable {
    /// The operation.
    public let operation: UUID
    /// The environment it worked on, when its resource ID reads as one.
    public let environment: UUID?
}
