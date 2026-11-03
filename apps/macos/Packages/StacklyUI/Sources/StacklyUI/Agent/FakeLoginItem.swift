#if DEBUG
    import Foundation
    import Synchronization

    /// An in-memory ``LoginItemControlling`` for tests, previews, and fixtures: it never calls
    /// `SMAppService` (spec 0032 AC-12). `register()` answers `afterRegister` and `unregister()`
    /// answers `.notRegistered`; both can fail with a queued error. Every call is logged.
    nonisolated final class FakeLoginItem: LoginItemControlling {
        /// One call the fake received.
        enum Call: Sendable, Equatable {
            case status, register, unregister, openLoginItems
        }

        private struct State {
            var status: LoginItemStatus
            var afterRegister: LoginItemStatus
            var failure: LoginItemError?
            var calls: [Call] = []
        }

        private let state: Mutex<State>

        /// Creates the fake.
        /// - Parameters:
        ///   - status: What `status()` answers at first.
        ///   - afterRegister: What `register()` leaves the status at.
        init(status: LoginItemStatus = .notRegistered, afterRegister: LoginItemStatus = .enabled) {
            state = Mutex(State(status: status, afterRegister: afterRegister))
        }

        /// The calls so far, in order.
        var calls: [Call] { state.withLock { $0.calls } }

        /// Makes the next `register()` or `unregister()` fail with `error`.
        func failNext(with error: LoginItemError) { state.withLock { $0.failure = error } }

        /// Changes the status the way the user does in System Settings.
        func setStatus(_ status: LoginItemStatus) { state.withLock { $0.status = status } }

        @concurrent func status() async -> LoginItemStatus {
            state.withLock {
                $0.calls.append(.status)
                return $0.status
            }
        }

        @concurrent func register() async throws(LoginItemError) -> LoginItemStatus {
            try change(.register) { $0.status = $0.afterRegister }
        }

        @concurrent func unregister() async throws(LoginItemError) -> LoginItemStatus {
            try change(.unregister) { $0.status = .notRegistered }
        }

        func openLoginItems() {
            state.withLock { $0.calls.append(.openLoginItems) }
        }

        private func change(_ call: Call, _ apply: (inout State) -> Void) throws(LoginItemError)
            -> LoginItemStatus
        {
            let result = state.withLock { state -> Result<LoginItemStatus, LoginItemError> in
                state.calls.append(call)
                if let failure = state.failure {
                    state.failure = nil
                    return .failure(failure)
                }
                apply(&state)
                return .success(state.status)
            }
            return try result.get()
        }
    }
#endif
