import Foundation
import Observation
import StacklyClient

/// What the model needs from the daemon link, so tests can drive it without
/// a socket.
public struct LinkHandle: Sendable {
    /// Every status change of the link.
    public var states: @Sendable () -> AsyncStream<ConnectionState>
    /// Starts the link.
    public var start: @Sendable () async -> Void
    /// Starts an attempt now when the link waits to retry.
    public var retryNow: @Sendable () async -> Void
    /// Sends one call on the open connection.
    public var call: @Sendable (Call, String?, CallOptions) async throws(ClientError) -> Reply
    /// Asks the connected daemon for its info and health again.
    public var refresh: @Sendable () async -> Void
    /// Closes the socket synchronously.
    public var stopNow: @Sendable () -> Void
    /// The link's status right now, which may be ahead of what ``states`` has
    /// delivered; `nil` when unknown.
    public var currentState: @Sendable () -> ConnectionState?

    /// Creates a handle from closures.
    public init(
        states: @escaping @Sendable () -> AsyncStream<ConnectionState>,
        start: @escaping @Sendable () async -> Void,
        retryNow: @escaping @Sendable () async -> Void,
        call: @escaping @Sendable (Call, String?, CallOptions) async throws(ClientError) -> Reply,
        refresh: @escaping @Sendable () async -> Void,
        stopNow: @escaping @Sendable () -> Void,
        currentState: @escaping @Sendable () -> ConnectionState? = { nil }
    ) {
        self.states = states
        self.start = start
        self.retryNow = retryNow
        self.call = call
        self.refresh = refresh
        self.stopNow = stopNow
        self.currentState = currentState
    }

    /// The handle of a real ``DaemonLink``.
    public static func live(_ link: DaemonLink) -> LinkHandle {
        LinkHandle(
            states: { link.states },
            start: { await link.start() },
            retryNow: { await link.retryNow() },
            call: { call, key, options throws(ClientError) in
                try await link.call(call, idempotencyKey: key, options: options)
            },
            refresh: { await link.refreshDaemon() },
            stopNow: { link.stopNow() },
            currentState: { link.currentState }
        )
    }
}

/// The daemon connection, presentation state, stores, and mutation availability.
@Observable
public final class DaemonModel {

    /// The presence.
    public private(set) var presence: DaemonPresence = .connecting(slow: false)
    /// What the shell shows for the presence.
    public private(set) var presentation: ShellPresentation
    /// When the last frame arrived after a completed snapshot, or `nil` when
    /// no snapshot ever completed.
    public private(set) var lastKnownAt: Date?
    /// The daemon last connected to, kept after the connection ends.
    public private(set) var lastDaemon: ConnectedDaemon?
    /// The last connection error.
    public private(set) var lastError: ErrorBody?
    /// The code of the last failed attempt while the status stays unreachable, kept while
    /// the stores refresh after it and `nil` in every other presence (spec 0015). It is
    /// needed because ``lastError`` is cleared when the connection opens.
    public private(set) var unreachableCode: String?
    /// When the link last completed a handshake.
    public private(set) var lastConnectedAt: Date?
    /// The link's latest status.
    public private(set) var connectionState: ConnectionState?

    /// The shell's stores.
    public let sync: ShellSync
    /// The operations that have not ended, for the Operations badge and subtitle.
    public let operations: OperationCenter
    /// The instances, for the Instances badge, subtitle, and list.
    public let instances: InstanceStore
    /// The provider catalog, for the Catalog page.
    public let catalog: CatalogStore
    /// The managed runtimes and the installs that have not ended, for the Runtimes page and
    /// the Get action of the Catalog (spec 0018).
    public let runtimes: RuntimeStore
    /// The space left on the disk that holds Stackly data, for the Overview (spec 0022).
    public let capacity: CapacityModel
    /// Start, Stop, and Retry of instances, shared by the Overview and the Instances page
    /// (spec 0022).
    public let instanceActions: InstanceActionModel
    /// The background agent, or `nil` when the shell has none (spec 0015).
    public let agent: BackgroundAgentModel?
    /// Whether `STACKLY_ROOT` is set. `ShellBootstrap.assemble` sets it once, right after
    /// the model is created; the agent model learned the same value at creation.
    public internal(set) var rootOverrideActive = false

    @ObservationIgnored private let link: LinkHandle
    @ObservationIgnored private let signals: AsyncStream<SyncSignal>?
    @ObservationIgnored private let endpoint: SocketEndpoint?
    @ObservationIgnored private let launchedAt: Date
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private let sleep: (Duration) async throws -> Void
    @ObservationIgnored private var inputs = DaemonPresence.Inputs()
    @ObservationIgnored private var statusBeforeConnect: ShellStatus
    /// The connection the stores refresh for, and the snapshot rounds completed
    /// before its first round started. Set by whichever comes first: the model
    /// handling `connected`, or the first snapshot signal of that connection,
    /// since the two arrive on separate streams.
    @ObservationIgnored private var roundStart: (connectedAt: Date, rounds: Int)?
    @ObservationIgnored private var refreshedOnce = false
    @ObservationIgnored private var tasks: [Task<Void, Never>] = []
    /// The ID of the daemon instance now, for the catalog's detail cache.
    @ObservationIgnored private let serverInstance = ServerInstanceBox()
    /// The mutation path the instance actions send through, filled once the model exists.
    @ObservationIgnored private let mutationPathBox: MutationPathBox

    /// Creates a model; call ``start()`` to connect.
    /// - Parameters:
    ///   - link: The daemon link.
    ///   - signals: The sync engine's signals, or `nil` without one.
    ///   - sync: The shell's stores.
    ///   - endpoint: The socket path, for Diagnostics, or `nil` when there is     none.
    ///   - launchedAt: When the app launched.
    ///   - now: The wall clock.
    ///   - sleep: Waits; tests inject one.
    ///   - agent: The background agent, or `nil` for a shell without one. The model attaches     it with its own clock and `sleep`.
    public init(
        link: LinkHandle,
        signals: AsyncStream<SyncSignal>?,
        sync: ShellSync = ShellSync(),
        endpoint: SocketEndpoint?,
        launchedAt: Date,
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) },

        agent: BackgroundAgentModel? = nil
    ) {
        self.link = link
        self.agent = agent
        self.signals = signals
        self.sync = sync
        self.endpoint = endpoint
        self.launchedAt = launchedAt
        self.now = now
        self.sleep = sleep
        let first = ShellStatus.connecting(slow: false, since: launchedAt)
        statusBeforeConnect = first
        presentation = ShellPresentation(status: first)
        operations = OperationCenter(
            call: { call throws(ClientError) in try await link.call(call, nil, .standard) },
            now: now)
        sync.register(operations.store)
        instances = InstanceStore(
            call: { call throws(ClientError) in try await link.call(call, nil, .standard) },
            now: now)
        sync.register(instances.store)
        let serverInstance = serverInstance
        catalog = CatalogStore(
            call: { call throws(ClientError) in try await link.call(call, nil, .standard) },
            serverInstance: { serverInstance.id },
            now: now)
        sync.register(catalog.store)
        capacity = CapacityModel(read: { call throws(ClientError) in
            try await link.call(call, nil, .standard)
        })
        let mutations = MutationPathBox()
        instanceActions = InstanceActionModel(
            send: { call, key in
                guard let path = mutations.path else {
                    return .notSent(ClientErrorCatalog.notConnected())
                }
                return await path.perform(call, idempotencyKey: key)
            },
            updates: { [operations] id in operations.updates(for: id) },
            readOperation: { id in
                do {
                    return try await OperationCenter.operation(
                        id,
                        call: { call throws(ClientError) in
                            try await link.call(call, nil, .standard)
                        })
                } catch {
                    return nil
                }
            },
            now: now)
        runtimes = RuntimeStore(
            call: { call throws(ClientError) in try await link.call(call, nil, .standard) },
            send: { call, key in
                guard let path = mutations.path else {
                    return .notSent(ClientErrorCatalog.notConnected())
                }
                return await path.perform(call, idempotencyKey: key)
            },
            updates: { [operations] id in operations.updates(for: id) },
            serverInstance: { serverInstance.id },
            now: now)
        sync.register(runtimes.store)
        mutationPathBox = mutations
        agent?.attach(
            presence: presence, now: now, sleep: sleep,

            systemRetry: { [weak self] in self?.systemRetry() })
        mutationPathBox.path = MutationPath(model: self)
    }

    /// Whether mutations can run, from the presentation.
    public var mutationAvailability: MutationAvailability {
        presentation.mutationAvailability
    }

    /// The counts behind the sidebar badges, each `nil` until its store loaded a
    /// snapshot.
    public var sidebarCounts: SidebarCounts {
        SidebarCounts(
            instances: instances.loadedCount, runningOperations: operations.runningCount)
    }

    /// Starts the link, the signal loop, and the one second timer of the
    /// first attempt.
    public func start() {
        guard tasks.isEmpty else { return }
        let states = link.states()
        tasks.append(
            Task { [weak self] in
                for await state in states {
                    self?.receive(state)
                }
            })
        if let signals {
            tasks.append(
                Task { [weak self] in
                    for await signal in signals {
                        self?.receive(signal)
                    }
                })
        }
        sync.onChange = { [weak self] in self?.update() }
        runtimes.follow(operations)
        tasks.append(
            Task { [weak self, sleep] in
                try? await sleep(.seconds(1))
                self?.firstAttemptIsSlow()
            })
        let link = self.link
        Task { await link.start() }
    }

    /// Stops listening; the link keeps running.
    public func stop() {
        for task in tasks { task.cancel() }
        tasks.removeAll()
        runtimes.stop()
        agent?.stop()
    }

    // MARK: - Actions

    /// Tries to connect now (Reconnect).
    public func reconnect() {
        if case .misconfigured = inputs.stable { return }
        let link = self.link
        Task { await link.retryNow() }
    }

    /// Asks the link to reconnect after a system event.
    public func systemRetry() {
        let link = self.link
        Task { await link.retryNow() }
    }

    /// Asks the connected daemon for its info and health again.
    public func refreshDiagnostics() {
        let link = self.link
        Task { await link.refresh() }
    }

    /// Sends a call that changes nothing outside a store, such as the detail
    /// of one item. It does not check the mutation gate.
    public func read(_ call: Call, options: CallOptions = .standard) async -> Result<
        Reply, ClientError
    > {
        do {
            return .success(try await link.call(call, nil, options))
        } catch {
            return .failure(error)
        }
    }

    /// The secret methods over this model's link (spec 0011), for the views that fill or
    /// reveal a secret.
    public var secrets: SecretsClient {
        let link = self.link
        return SecretsClient { call, options throws(ClientError) in
            try await link.call(call, nil, options)
        }
    }

    /// Whether the link holds an open connection.
    var isLinked: Bool {
        if case .connected = connectionState { return true }
        return false
    }

    /// Sends a mutation on the open connection.
    func send(_ call: Call, idempotencyKey: String) async throws(ClientError) -> Reply {
        try await link.call(call, idempotencyKey, .standard)
    }

    /// Closes the socket synchronously, for app termination.
    func stopNow() {
        agent?.stop()
        link.stopNow()
    }

    // MARK: - Diagnostics

    /// The Connection section of Diagnostics.
    public var diagnostics: ConnectionDiagnostics {
        ConnectionDiagnostics(
            status: presentation.status,
            socketPath: endpoint?.socketPath,
            lastError: lastError.map(Self.diagnosticsError),
            nextAttempt: ShellStatusMapping.nextAttempt(of: connectionState),
            lastConnectedAt: lastConnectedAt,
            daemon: lastDaemon?.info.map {
                DaemonFacts(
                    version: $0.version, pid: Int($0.pid), startedAt: $0.startedAt,
                    serverInstanceId: $0.serverInstanceId.uuidString.lowercased())
            },
            daemonProtocol: daemonProtocol,
            appProtocol: ShellStatusMapping.appProtocol,
            contractDrift: lastDaemon?.accepted.contractDrift,
            health: lastDaemon?.health.map(Self.healthFacts),
            agentHint: agentHint
        )
    }

    /// The text Copy Details puts on the pasteboard, with the system calendar,
    /// locale, and time zone.
    /// - Parameter now: The time the agent hint is worked out at.
    public func copyDiagnostics(now: Date = Date()) -> String {
        ConnectionDiagnostics.copyText(
            diagnostics.rows(
                calendar: .current, locale: .current, timeZone: .current, now: now))
    }

    // MARK: - The agent in the banner

    /// What the window banner is built from, or `nil` for a shell without an agent.
    public var agentBannerInput: AgentBanner.Input? {
        guard let agent else { return nil }
        return AgentBanner.Input(
            status: presentation.status, failureCode: unreachableCode,
            assessment: agent.assessment, detail: agent.unavailableDetail,
            isStarting: agent.isStarting)
    }

    /// The window banner at `now`: the agent's row while the daemon is unreachable with one of the two connection codes, otherwise the banner of the status.
    public func bannerContent(now: Date) -> DaemonBannerContent? {
        agentBannerInput.flatMap { AgentBanner.content($0, now: now) } ?? presentation.banner
    }

    /// The hint of the Connection section: the banner input, only while the presence is
    /// `unreachable` with one of the two codes.
    private var agentHint: AgentBanner.Input? {
        guard case .unreachable = presence, let input = agentBannerInput,
            let code = input.failureCode, AgentBanner.failureCodes.contains(code)
        else {
            return nil
        }
        return input
    }

    private var daemonProtocol: String? {
        if case .incompatible(.majorMismatch(let server, _), _, _) = connectionState {
            return ShellStatusMapping.text(server)
        }
        return lastDaemon.map { ShellStatusMapping.text($0.accepted.protocol) }
    }

    static func diagnosticsError(_ body: ErrorBody) -> DiagnosticsError {
        DiagnosticsError(
            code: body.code, category: body.category.rawValue, message: body.message,
            reason: body.detail("reason"), operation: body.detail("op"), kind: body.detail("kind"),
            method: body.detail("method"), daemonHint: body.hint)
    }

    static func healthFacts(_ health: DaemonHealth) -> HealthFacts {
        HealthFacts(
            status: health.status.rawValue,
            subsystems: (health.subsystems ?? [])
                .filter { $0.status != .ok && $0.status != .notBuilt }
                .map { .init(name: $0.name, status: $0.status.rawValue, detail: $0.detail) })
    }

    // MARK: - Updates

    private func firstAttemptIsSlow() {
        guard inputs.stable == nil else { return }
        inputs.slow = true
        update()
    }

    func receive(_ state: ConnectionState) {
        connectionState = state
        switch state {
        case .connecting:
            break
        case .connected(let daemon):
            if case .connected(let previous) = inputs.stable,
                previous.connectedAt == daemon.connectedAt
            {
                // A heartbeat reply is a frame too.
                if inputs.refreshed { lastKnownAt = now() }
            } else {
                statusBeforeConnect = presentation.status
                noteRoundStart(for: daemon.connectedAt)
                lastConnectedAt = daemon.connectedAt
            }
            lastDaemon = daemon
            serverInstance.id = daemon.info?.serverInstanceId
            // The error describes a failed attempt, so a handshake that
            // succeeded ends it and the hint that came with it.
            lastError = nil
            inputs.stable = state
            // The heartbeat brings the latest health, so recovery mode
            // follows it in both directions.
            inputs.recovery = daemon.health?.recovery != nil
        case .unreachable(let error, _, _):
            lastError = error
            unreachableCode = error.code
            inputs.stable = state
        case .untrusted(let error, _, _):
            lastError = error
            unreachableCode = nil
            inputs.stable = state
        case .incompatible(let reason, _, _):
            lastError = reason.error
            unreachableCode = nil
            inputs.stable = state
        case .misconfigured:
            unreachableCode = nil
            inputs.stable = state
        }
        update()
    }

    func receive(_ signal: SyncSignal) {
        switch signal {
        case .resnapshot, .caughtUp:
            // The signal belongs to the link's current connection, which the
            // states stream may not have delivered yet.
            if case .connected(let daemon) = link.currentState() {
                noteRoundStart(for: daemon.connectedAt)
            }
        case .event, .lastFrameAt:
            break
        }
        sync.handle(signal)
        if case .lastFrameAt(let date) = signal, inputs.refreshed {
            lastKnownAt = date
        }
        update()
    }

    /// Records the rounds completed before the first round of the connection
    /// made at `connectedAt`, once per connection.
    private func noteRoundStart(for connectedAt: Date) {
        guard roundStart?.connectedAt != connectedAt else { return }
        roundStart = (connectedAt, sync.completedRounds)
    }

    func update() {
        let wasRefreshed = inputs.refreshed
        if let start = roundStart, case .connected(let daemon) = inputs.stable,
            start.connectedAt == daemon.connectedAt
        {
            inputs.refreshed = sync.isSettled && sync.completedRounds > start.rounds
        } else {
            inputs.refreshed = false
        }
        if inputs.refreshed, wasRefreshed == false {
            refreshedOnce = true
            lastKnownAt = now()
        }
        inputs.lastKnownAt = refreshedOnce ? lastKnownAt : nil
        let next = DaemonPresence.derive(inputs)
        switch next {
        case .unreachable, .refreshing: break
        default: unreachableCode = nil
        }
        let status = ShellStatusMapping.status(
            for: next, stable: inputs.stable, launchedAt: launchedAt, before: statusBeforeConnect)
        let attempting: Bool =
            switch (connectionState, inputs.stable) {
            case (.connecting, .some(let stable)): Self.isFailure(stable)
            default: false
            }
        presentation = ShellPresentation(
            status: status, lastErrorCode: lastError?.code, isReconnecting: attempting)
        let previous = presence
        if next != previous {
            presence = next
        }
        if next != previous { agent?.presenceDidChange(next) }
    }

    private static let pollSeconds: TimeInterval = 5

    private static func isFailure(_ state: ConnectionState) -> Bool {
        switch state {
        case .unreachable, .incompatible, .untrusted: true
        case .connecting, .connected, .misconfigured: false
        }
    }
}

/// The ID of the daemon instance the link talks to, shared with the catalog's detail cache.
private final class ServerInstanceBox {
    var id: UUID?
}

/// Holds the model's mutation path for the closures built before the model is complete.
@MainActor private final class MutationPathBox {
    var path: MutationPath?
}
