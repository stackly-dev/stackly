import Foundation
import Observation
import StacklyClient

/// The state behind the menu bar item and its popover (spec 0032, *Popover menu bar*): what the
/// icon looks like, the header, the Services rows, the operations that run, and the Stop All
/// flow. It reads the ``DaemonModel`` the shell already has, so it opens no connection of its
/// own, and sends nothing when the popover closes or the app quits.
@MainActor
@Observable
public final class MenuBarModel {
    /// Where the Stop All flow stands. The popover draws the one that shows.
    public enum StopAllPhase: Equatable {
        /// Nothing was asked.
        case idle
        /// The preview is being read.
        case loadingPreview
        /// The preview the person decides on. `changed` is true when it replaced one that the
        /// daemon refused as out of date.
        case preview(StopAllPreview, changed: Bool)
        /// The confirmed Stop All runs.
        case stopping(StopAllPreview)
        /// Stop All ended.
        case result(StopAllPreview, StopAllResult)
        /// The preview or the stop could not be done; the sentence says why.
        case failed(String)
    }

    /// Where the Stop All flow stands.
    public private(set) var phase: StopAllPhase = .idle

    @ObservationIgnored private let daemon: DaemonModel
    @ObservationIgnored private let previewStopAll: () async throws(ClientError) -> StopAllPreview
    @ObservationIgnored private let stopAll:
        ([UUID], String) async throws(ClientError) -> StopAllResult
    @ObservationIgnored private let connected: (@MainActor () -> Bool)?
    @ObservationIgnored private var task: Task<Void, Never>?

    /// Creates the model over the shell's daemon model.
    /// - Parameters:
    ///   - daemon: The shell's daemon model.
    ///   - previewStopAll: Reads `instance.preview_stop_all`; the daemon's by default.
    ///   - stopAll: Sends `instance.stop_all` with the confirmed IDs and a key; the daemon's by
    ///     default.
    ///   - connected: Whether the presence is `connected`; the daemon model's by default.
    public init(
        daemon: DaemonModel,
        connected: (@MainActor () -> Bool)? = nil,
        previewStopAll: (() async throws(ClientError) -> StopAllPreview)? = nil,
        stopAll: (([UUID], String) async throws(ClientError) -> StopAllResult)? = nil
    ) {
        self.daemon = daemon
        self.connected = connected
        let client = daemon.stopAllClient
        self.previewStopAll =
            previewStopAll ?? { () throws(ClientError) in
                try await client.previewStopAll()
            }
        self.stopAll =
            stopAll ?? { (ids, key) throws(ClientError) in
                try await client.stopAll(expectedInstanceIds: ids, idempotencyKey: key)
            }
    }

    // MARK: - Icon and header

    /// Whether the presence is `connected`.
    public var isConnected: Bool {
        if let connected { return connected() }
        if case .connected = daemon.presence { return true }
        return false
    }

    /// The instances the stores hold: the last known state while the daemon is away.
    private var instances: [InstanceSummary] { daemon.instances.instances }

    /// How many instances are ready, by the rule of the Overview card Running.
    public var runningCount: Int { InstanceCounts(of: instances).ready }

    /// How many instances have the lifecycle `failed`.
    public var failedCount: Int { InstanceCounts(of: instances).failed }

    /// The icon variant.
    public var iconVariant: MenuBarIconVariant {
        .of(isConnected: isConnected, failedCount: failedCount)
    }

    /// The daemon status text of the sidebar footer.
    public var statusText: String { daemon.presentation.footer.label }

    // MARK: - Services

    /// The instances the section lists, at most eight.
    public var rows: [MenuBarRow] {
        let operating = instancesWithOperation
        return MenuBarRows.ordered(instances).prefix(MenuBarRows.limit).map { summary in
            MenuBarRow(
                summary: summary,
                endpoint: InstanceText.ports(summary.endpoints),
                appearance: StatusAdapter.appearance(of: summary),
                button: MenuBarRows.button(
                    for: summary, isConnected: isConnected,
                    hasOtherOperation: operating.contains(summary.id)
                        || daemon.instanceActions.localStatus(for: summary.id) != .none))
        }
    }

    /// How many instances there are beyond the eight rows, or zero.
    public var hiddenCount: Int { max(0, MenuBarRows.ordered(instances).count - MenuBarRows.limit) }

    /// The number the "Show all" row names.
    public var totalCount: Int { MenuBarRows.ordered(instances).count }

    /// The instances an unfinished operation works on.
    private var instancesWithOperation: Set<UUID> {
        Set(
            daemon.operations.active.compactMap { operation -> UUID? in
                guard let resource = operation.resource, resource.kind == .instance else {
                    return nil
                }
                return UUID(uuidString: resource.id)
            })
    }

    /// Starts or stops the instance of a row, as the buttons of the instance page do: no
    /// confirmation, and the stop is the manual one (spec 0031, AC-8).
    /// - Parameter row: The row whose button was clicked.
    public func perform(_ row: MenuBarRow) {
        guard row.button.isEnabled else { return }
        daemon.instanceActions.perform(
            row.button.verb, for: row.summary,
            context: InstanceActionContext(report: { _ in }))
    }

    // MARK: - Operations

    /// The operations that run, with the kinds the popover lists, newest first.
    public var operationRows: [MenuBarOperation] {
        let names = Dictionary(
            instances.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first })
        let runtimes = OperationTitle.RuntimeNames(store: daemon.runtimes)
        return daemon.operations.active.compactMap { operation in
            guard Self.listedKinds.contains(operation.kind) else { return nil }
            return MenuBarOperation(
                id: operation.id,
                title: OperationTitle.title(
                    for: operation, instanceNames: names, runtimes: runtimes),
                percent: Self.percent(of: operation.progress))
        }
    }

    /// The operation kinds the popover lists.
    static let listedKinds: Set<OperationKind> = [
        .installRuntime, .createInstance, .startInstance, .stopInstance, .restartInstance,
        .stopAllInstances,
    ]

    /// The percent of a progress, or `nil` when the operation does not know it.
    static func percent(of progress: OperationProgress) -> Int? {
        guard case .counted(let counted) = progress, counted.total > 0 else { return nil }
        return Int((Double(min(counted.done, counted.total)) / Double(counted.total)) * 100)
    }

    // MARK: - Stop All

    /// Whether Stop All Services… can be asked.
    public var canBeginStopAll: Bool {
        isConnected && phase == .idle
    }

    /// Why Stop All Services… is off, or `nil`.
    public var stopAllDisabledReason: String? {
        isConnected ? nil : MenuBarCopy.unreachable
    }

    /// Reads the preview and shows it in the popover.
    public func beginStopAll() {
        guard canBeginStopAll else { return }
        phase = .loadingPreview
        task = Task { [weak self] in await self?.loadPreview(changed: false) }
    }

    /// Closes the preview or the result without stopping anything.
    public func dismissStopAll() {
        task?.cancel()
        task = nil
        if case .stopping = phase { return }
        phase = .idle
    }

    /// Drops a finished Stop All, its result or its failure, when the popover opens again.
    public func clearFinishedStopAll() {
        switch phase {
        case .result, .failed: phase = .idle
        default: break
        }
    }

    /// Stops what the preview shows, and only that: the IDs go to the daemon, which refuses
    /// when the instances it would stop differ (spec 0032, AC-7 and AC-8).
    public func confirmStopAll() {
        guard case .preview(let preview, _) = phase, preview.items.isEmpty == false,
            isConnected
        else { return }
        phase = .stopping(preview)
        let ids = preview.items.map(\.instanceId)
        let key = MutationPath.newKey()
        task = Task { [weak self] in await self?.run(preview: preview, ids: ids, key: key) }
    }

    private func loadPreview(changed: Bool) async {
        do {
            let preview = try await previewStopAll()
            guard Task.isCancelled == false else { return }
            phase = .preview(preview, changed: changed)
        } catch {
            guard Task.isCancelled == false else { return }
            phase = .failed(Self.sentence(for: error))
        }
    }

    private func run(preview: StopAllPreview, ids: [UUID], key: String) async {
        do {
            let result = try await stopAll(ids, key)
            phase = .result(preview, result)
        } catch {
            if case .rejected(let body) = error, body.code == "instance.stop_all_changed" {
                // Nothing stopped. The list is read again, for one more confirmation.
                phase = .loadingPreview
                await loadPreview(changed: true)
                return
            }
            phase = .failed(Self.sentence(for: error))
        }
    }

    private static func sentence(for error: ClientError) -> String {
        switch error {
        case .rejected(let body): DaemonText.sentence(body.message)
        case .notSent: MutationFeedback.notSent
        case .outcomeUnknown: MutationFeedback.outcomeUnknown
        }
    }
}

/// One row of the Services section.
public struct MenuBarRow: Identifiable, Equatable {
    /// The instance ID.
    public var id: UUID { summary.id }
    /// The instance.
    public let summary: InstanceSummary
    /// The endpoint text of the Instances table.
    public let endpoint: String
    /// The status label.
    public let appearance: StatusAppearance
    /// The Start or Stop button.
    public let button: MenuBarRowButton

    /// The instance name.
    public var name: String { summary.name }
}

/// One operation that runs, as a row of the popover.
public struct MenuBarOperation: Identifiable, Equatable, Sendable {
    /// The operation ID.
    public let id: UUID
    /// The title, such as "Start orders-db".
    public let title: String
    /// The percent, or `nil` when the operation does not know it.
    public let percent: Int?
}
