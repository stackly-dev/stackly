#if DEBUG
    import Foundation
    import StacklyClient
    import Synchronization

    /// A daemon link that plays a ``FixtureScenario``: it opens no socket, starts no
    /// process, and reads no environment (spec 0068).
    ///
    /// Reads are answered from the scenario's data. A mutation is answered by the
    /// scenario's reply for that method, or fails with `ipc.not_connected` and nothing sent,
    /// exactly like the real app while it is not connected.
    public nonisolated final class FixtureLink: Sendable {
        /// The methods the shell's stores call to load a snapshot. A store that a later
        /// feature registers with `ShellSync` adds its `list` method here.
        static let snapshotMethods: Set<String> = [
            "instance.list", "operation.list", "catalog.list_providers", "runtime.list",
        ]

        private struct State {
            var current: ConnectionState?
            var started = false
            var isHeld = false
            var pending: ConnectionState?
            var answered: Set<String> = []
            var isRefreshed: (@Sendable () async -> Bool)?
        }

        /// The scenario this link plays.
        public let scenario: FixtureScenario

        private let state = Mutex(State())
        private let statuses: AsyncStream<ConnectionState>
        private let statusContinuation: AsyncStream<ConnectionState>.Continuation
        private let signalStream: AsyncStream<SyncSignal>
        private let signalContinuation: AsyncStream<SyncSignal>.Continuation

        /// Creates a link for `scenario`; nothing is reported until ``start()``.
        public init(_ scenario: FixtureScenario) {
            self.scenario = scenario
            (statuses, statusContinuation) = AsyncStream.makeStream(of: ConnectionState.self)
            (signalStream, signalContinuation) = AsyncStream.makeStream(of: SyncSignal.self)
        }

        /// The sync engine's signals: a full snapshot request and the time of a frame each
        /// time the script connects.
        public var signals: AsyncStream<SyncSignal> {
            signalStream
        }

        /// Tells the link how to see that the shell finished the snapshot, so a status that
        /// follows the snapshot is reported after the shell took the data.
        public func attach(isRefreshed: @escaping @Sendable () async -> Bool) {
            state.withLock { $0.isRefreshed = isRefreshed }
        }

        /// Plays the script: the statuses up to the first wait, in order.
        public func start() {
            let shouldStart = state.withLock { state -> Bool in
                guard state.started == false else { return false }
                state.started = true
                for step in scenario.connection {
                    switch step {
                    case .afterSnapshot(let next): state.pending = next
                    case .holdSnapshot: state.isHeld = true
                    case .state: break
                    }
                }
                return true
            }
            guard shouldStart else { return }
            for case .state(let next) in scenario.connection {
                report(next)
            }
        }

        /// Ends the streams.
        public func stop() {
            statusContinuation.finish()
            signalContinuation.finish()
        }

        /// The link's status now.
        public var currentState: ConnectionState? {
            state.withLock { $0.current }
        }

        fileprivate var statusStream: AsyncStream<ConnectionState> {
            statuses
        }

        private func report(_ next: ConnectionState) {
            state.withLock { $0.current = next }
            statusContinuation.yield(next)
            if case .connected = next {
                signalContinuation.yield(.resnapshot(.noCursor))
                signalContinuation.yield(.lastFrameAt(FixtureClock.now))
            }
        }

        // MARK: - Calls

        fileprivate func call(_ call: Call) async throws(ClientError) -> Reply {
            guard case .connected = currentState else {
                throw .notSent(ClientErrorCatalog.notConnected())
            }
            let method = call.methodName
            let isSnapshot = Self.snapshotMethods.contains(method)
            if isSnapshot, state.withLock({ $0.isHeld }) {
                while Task.isCancelled == false {
                    try? await Task.sleep(for: .seconds(3600))
                }
                throw .notSent(ClientErrorCatalog.notConnected())
            }
            let reply = try answer(call)
            if isSnapshot { noteAnswered(method) }
            return reply
        }

        private func answer(_ call: Call) throws(ClientError) -> Reply {
            switch call {
            case .instanceList:
                return .instanceList(InstancePage(items: scenario.instances, next: nil))
            case .instanceGet(let params):
                guard let found = scenario.instances.first(where: { $0.id == params.instanceId })
                else {
                    throw .rejected(
                        .fixtureNotFound(.instance, id: params.instanceId.uuidString.lowercased()))
                }
                return .instanceGet(found)
            case .instanceGetDetail(let params):
                guard let found = scenario.instances.first(where: { $0.id == params.instanceId })
                else {
                    throw .rejected(
                        .fixtureNotFound(.instance, id: params.instanceId.uuidString.lowercased()))
                }
                return .instanceGetDetail(FixtureDetail.detail(of: found))
            case .instanceGetDataUsage(let params):
                guard let found = scenario.instances.first(where: { $0.id == params.instanceId })
                else {
                    throw .rejected(
                        .fixtureNotFound(.instance, id: params.instanceId.uuidString.lowercased()))
                }
                return .instanceGetDataUsage(DataUsageReply(usage: FixtureDetail.usage(of: found)))
            case .operationList(let params):
                let items = scenario.operations.filter { operation in
                    (params.activeOnly != true || Self.hasNotEnded(operation.state))
                        && (params.resource == nil || params.resource == operation.resource)
                }
                return .operationList(OperationPage(items: items, next: nil))
            case .operationGet(let params):
                guard let found = scenario.operations.first(where: { $0.id == params.operationId })
                else {
                    throw .rejected(
                        .fixtureNotFound(.operation, id: params.operationId.uuidString.lowercased())
                    )
                }
                return .operationGet(OperationDetail(operation: found, steps: nil, created: nil))
            case .catalogListProviders:
                return .catalogListProviders(
                    ListProvidersReply(catalog: .fixture, items: scenario.providers))
            case .runtimeList:
                return runtimeList()
            case .runtimeGet(let params):
                return try runtimeGet(params.runtimeId)
            case .daemonGetInfo:
                if let info = scenario.daemonInfo { return .daemonGetInfo(info) }
            case .daemonGetHealth:
                if let health = scenario.health { return .daemonGetHealth(health) }
            default:
                break
            }
            switch scenario.replies[call.methodName] {
            case .reply(let reply): return reply
            case .error(let error): throw error
            case nil: throw .notSent(ClientErrorCatalog.notConnected())
            }
        }

        /// `runtime.list`: the scenario's reply, or a page with no runtime and no install.
        private func runtimeList() -> Reply {
            if case .reply(let reply) = scenario.replies["runtime.list"] { return reply }
            return .runtimeList(RuntimeListPage(items: [], next: nil, installing: []))
        }

        /// `runtime.get`: the runtime of the scenario's list with that ID, with the detail
        /// the fixtures give it.
        private func runtimeGet(_ id: UUID) throws(ClientError) -> Reply {
            guard case .runtimeList(let page) = runtimeList(),
                let found = (page.items ?? []).first(where: { $0.id == id })
            else {
                throw .rejected(.fixtureNotFound(.runtime, id: id.uuidString.lowercased()))
            }
            return .runtimeGet(FixtureRuntime.detail(of: found))
        }

        /// Whether an operation in `state` has not ended: what `activeOnly` returns.
        private static func hasNotEnded(_ state: OperationState) -> Bool {
            switch state {
            case .queued, .running, .cancelRequested, .needsRecovery: true
            case .succeeded, .failed, .cancelled: false
            }
        }

        /// Records that a store's `list` call was answered; once every store's was, reports
        /// the status that follows the snapshot.
        private func noteAnswered(_ method: String) {
            let next = state.withLock { state -> ConnectionState? in
                state.answered.insert(method)
                guard Self.snapshotMethods.isSubset(of: state.answered), let next = state.pending
                else { return nil }
                state.pending = nil
                return next
            }
            guard let next else { return }
            Task {
                await waitUntilRefreshed()
                report(next)
            }
        }

        /// Waits until the shell took the snapshot, so the status that follows finds the
        /// shell refreshed once; gives up after about 4 seconds.
        private func waitUntilRefreshed() async {
            guard let isRefreshed = state.withLock({ $0.isRefreshed }) else { return }
            for _ in 0..<2000 {
                if await isRefreshed() { return }
                try? await Task.sleep(for: .milliseconds(2))
            }
        }
    }

    extension LinkHandle {
        /// A handle that plays `scenario` on its own ``FixtureLink``.
        public static func fixture(_ scenario: FixtureScenario) -> LinkHandle {
            fixture(FixtureLink(scenario))
        }

        /// A handle over `link`, for the caller that also needs its signals.
        public static func fixture(_ link: FixtureLink) -> LinkHandle {
            LinkHandle(
                states: { link.statusStream },
                start: { link.start() },
                retryNow: {},
                call: { call, _, _ throws(ClientError) in try await link.call(call) },
                refresh: {},
                stopNow: { link.stop() },
                currentState: { link.currentState }
            )
        }
    }
#endif
