#if DEBUG
    import Foundation
    import Synchronization

    /// An in-memory ``NotificationPosting`` for tests, previews, and fixtures: it never touches
    /// `UNUserNotificationCenter`, so a test never asks macOS for permission (spec 0032 AC-22).
    /// `requestPermission()` answers `answer`. Every call is logged in order.
    nonisolated final class FakeNotificationPoster: NotificationPosting {
        /// One call the fake received.
        enum Call: Sendable, Equatable {
            case permission
            case requestPermission
            case post(AppNotification)
            case openSystemSettings
            case installClickHandler
        }

        private struct State {
            var permission: NotificationPermission
            var answer: NotificationPermission
            var calls: [Call] = []
            var handler: (@MainActor @Sendable (NotificationTarget) -> Void)?
        }

        private let state: Mutex<State>

        /// Creates the fake.
        /// - Parameters:
        ///   - permission: What `permission()` answers at first.
        ///   - answer: What a request leaves the permission at.
        init(
            permission: NotificationPermission = .notDetermined,
            answer: NotificationPermission = .authorized
        ) {
            state = Mutex(State(permission: permission, answer: answer))
        }

        /// The calls so far, in order.
        var calls: [Call] { state.withLock { $0.calls } }

        /// The notifications posted so far, in order.
        var posted: [AppNotification] {
            calls.compactMap { call in
                if case .post(let notification) = call { notification } else { nil }
            }
        }

        /// Changes the permission the way the person does in System Settings.
        func setPermission(_ permission: NotificationPermission) {
            state.withLock { $0.permission = permission }
        }

        /// Plays a click on a notification that opens `target`.
        @MainActor func click(_ target: NotificationTarget) {
            let handler = state.withLock { $0.handler }
            handler?(target)
        }

        @concurrent func permission() async -> NotificationPermission {
            state.withLock {
                $0.calls.append(.permission)
                return $0.permission
            }
        }

        @concurrent func requestPermission() async -> NotificationPermission {
            state.withLock {
                $0.calls.append(.requestPermission)
                if $0.permission == .notDetermined { $0.permission = $0.answer }
                return $0.permission
            }
        }

        @concurrent func post(_ notification: AppNotification) async {
            state.withLock { $0.calls.append(.post(notification)) }
        }

        func openSystemSettings() {
            state.withLock { $0.calls.append(.openSystemSettings) }
        }

        func installClickHandler(
            _ handler: @escaping @MainActor @Sendable (NotificationTarget) -> Void
        ) {
            state.withLock {
                $0.calls.append(.installClickHandler)
                $0.handler = handler
            }
        }
    }
#endif
