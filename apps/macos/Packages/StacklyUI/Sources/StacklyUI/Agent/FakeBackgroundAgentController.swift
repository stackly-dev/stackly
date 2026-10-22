#if DEBUG
    import Foundation
    import Synchronization

    /// An in-memory ``BackgroundAgentControlling`` for tests, previews, and fixtures: it
    /// never calls `SMAppService` (spec 0015, *GUI*).
    ///
    /// `status()` answers `status`. `register()` and `relaunch()` answer `afterRegister`,
    /// and from then on `status()` answers it too; `unregister()` answers
    /// `.registration(.notRegistered, …)` and `status()` follows. Both answers can be
    /// replaced at any time. Every call is logged in order, and a gate can hold the next
    /// answer until the test releases it.
    nonisolated final class FakeBackgroundAgentController: BackgroundAgentControlling {
        /// One call the fake received.
        enum Call: Sendable, Equatable {
            case status, register, unregister, relaunch, openApprovalSettings
        }

        /// The bundle every default answer names, as in the fixtures of spec 0068.
        static let fixtureBundle = "/fixture/Applications/Stackly.app"

        private struct State {
            var status: Result<AgentProbe, AgentActionError>
            var afterRegister: Result<AgentProbe, AgentActionError>
            var calls: [Call] = []
            var holdNext = false
            var held: CheckedContinuation<Void, Never>?
        }

        private let state: Mutex<State>

        /// Creates the fake.
        ///
        /// - Parameters:
        ///   - status: The answer of `status()` at first; a fresh install, `notFound`.
        ///   - afterRegister: The answer of `register()` and `relaunch()`, and of
        ///     `status()` after them (when it is not an error); `requiresApproval`.
        init(
            status: Result<AgentProbe, AgentActionError> = .success(
                .registration(.notFound, bundle: FakeBackgroundAgentController.fixtureBundle)),
            afterRegister: Result<AgentProbe, AgentActionError> = .success(
                .registration(
                    .requiresApproval, bundle: FakeBackgroundAgentController.fixtureBundle))
        ) {
            state = Mutex(State(status: status, afterRegister: afterRegister))
        }

        /// The calls so far, in order.
        var calls: [Call] { state.withLock { $0.calls } }

        /// The number of calls of `call`.
        func count(of call: Call) -> Int { calls.filter { $0 == call }.count }

        /// Replaces the answer of `status()`.
        func setStatus(_ answer: Result<AgentProbe, AgentActionError>) {
            state.withLock { $0.status = answer }
        }

        /// Replaces the answer of `register()`, `relaunch()`, and `status()` after them.
        func setAfterRegister(_ answer: Result<AgentProbe, AgentActionError>) {
            state.withLock { $0.afterRegister = answer }
        }

        /// Makes the next call wait, after it picked its answer, until ``release()``.
        func holdNextCall() {
            state.withLock { $0.holdNext = true }
        }

        /// Whether a call waits at the gate.
        var isHolding: Bool { state.withLock { $0.held != nil } }

        /// Lets the waiting call return its answer.
        func release() {
            let held = state.withLock { state in
                defer { state.held = nil }
                return state.held
            }
            held?.resume()
        }

        /// Suspends the test until a call waits at the gate.
        func waitUntilHolding() async {
            while isHolding == false { await Task.yield() }
        }

        // MARK: - BackgroundAgentControlling

        @concurrent func status() async throws(AgentActionError) -> AgentProbe {
            let answer = state.withLock { state in
                state.calls.append(.status)
                return state.status
            }
            await gate()
            return try answer.get()
        }

        @concurrent func register() async throws(AgentActionError) -> AgentProbe {
            try await registering(.register)
        }

        @concurrent func relaunch() async throws(AgentActionError) -> AgentProbe {
            try await registering(.relaunch)
        }

        @concurrent func unregister() async throws(AgentActionError) -> AgentProbe {
            let answer = state.withLock { state in
                state.calls.append(.unregister)
                let bundle = state.afterRegister.bundle ?? Self.fixtureBundle
                state.status = .success(.registration(.notRegistered, bundle: bundle))
                return state.status
            }
            await gate()
            return try answer.get()
        }

        func openApprovalSettings() {
            state.withLock { $0.calls.append(.openApprovalSettings) }
        }

        private func registering(_ call: Call) async throws(AgentActionError) -> AgentProbe {
            let answer = state.withLock { state in
                state.calls.append(call)
                // A refused registration leaves the status as it was.
                if case .success = state.afterRegister { state.status = state.afterRegister }
                return state.afterRegister
            }
            await gate()
            return try answer.get()
        }

        private func gate() async {
            let hold = state.withLock { state in
                defer { state.holdNext = false }
                return state.holdNext
            }
            guard hold else { return }
            await withCheckedContinuation { continuation in
                state.withLock { $0.held = continuation }
            }
        }
    }

    extension Result where Success == AgentProbe, Failure == AgentActionError {
        /// The bundle of the probe, when the answer is one.
        nonisolated fileprivate var bundle: String? {
            switch self {
            case .success(.registration(_, let bundle)), .success(.helperMissing(let bundle)):
                bundle
            default:
                nil
            }
        }
    }

    /// An in-memory ``LaunchRecordReading`` that answers one status until a test changes
    /// it. It never reads the user's files.
    nonisolated final class FakeLaunchRecordReader: LaunchRecordReading {
        private let state: Mutex<(status: LaunchRecordStatus, reads: Int)>

        /// Creates the reader.
        ///
        /// - Parameter status: What it answers.
        init(_ status: LaunchRecordStatus = .missing) {
            state = Mutex((status, 0))
        }

        /// A reader for a record launched at `lastLaunchedAt`, or no record when `nil`.
        convenience init(launchedAt lastLaunchedAt: Date?) {
            self.init(
                lastLaunchedAt.map { LaunchRecordStatus(exists: true, lastLaunchedAt: $0) }
                    ?? .missing)
        }

        /// How many times the record was read.
        var reads: Int { state.withLock { $0.reads } }

        /// Replaces the answer.
        func set(_ status: LaunchRecordStatus) {
            state.withLock { $0.status = status }
        }

        @concurrent func read() async -> LaunchRecordStatus {
            state.withLock { state in
                state.reads += 1
                return state.status
            }
        }
    }
#endif
