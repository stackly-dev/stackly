import Foundation

/// Whether macOS lets Stackly post notifications (spec 0032 AC-22).
nonisolated public enum NotificationPermission: Sendable, Equatable {
    /// The person was never asked.
    case notDetermined
    /// The person said no, in the prompt or in System Settings.
    case denied
    /// The person said yes.
    case authorized
}

/// What a notification opens when the person clicks it (spec 0032, *Pemicu notifikasi*).
nonisolated public enum NotificationTarget: Sendable, Equatable {
    /// The detail page of an instance.
    case instance(UUID)
    /// The Operations page.
    case operations
    /// The Diagnostics page.
    case diagnostics

    /// The values that travel with the notification, as strings.
    var userInfo: [String: String] {
        switch self {
        case .instance(let id): ["target": "instance", "id": id.uuidString]
        case .operations: ["target": "operations"]
        case .diagnostics: ["target": "diagnostics"]
        }
    }

    /// The target in `userInfo`, or `nil` when it holds none Stackly wrote.
    init?(userInfo: [String: String]) {
        switch userInfo["target"] {
        case "instance":
            guard let id = userInfo["id"].flatMap(UUID.init(uuidString:)) else { return nil }
            self = .instance(id)
        case "operations": self = .operations
        case "diagnostics": self = .diagnostics
        default: return nil
        }
    }

    /// The page of the main window the target opens.
    var destination: ShellDestination {
        switch self {
        case .instance(let id): .instance(id)
        case .operations: .operations
        case .diagnostics: .diagnostics
        }
    }
}

/// One notification Stackly asks macOS to show. It never holds a secret, a path, or a line of a
/// log (spec 0032, *Notifikasi*).
nonisolated public struct AppNotification: Sendable, Equatable {
    /// Stops macOS from showing the same notification twice.
    public let identifier: String
    /// The title.
    public let title: String
    /// The text under the title.
    public let body: String
    /// What a click opens.
    public let target: NotificationTarget

    /// Creates a notification.
    public init(identifier: String, title: String, body: String, target: NotificationTarget) {
        self.identifier = identifier
        self.title = title
        self.body = body
        self.target = target
    }
}

/// The macOS side of notifications, as the app sees it (spec 0032, *Notifikasi*). The live one
/// is ``UserNotificationPoster``, and this folder is the only one that imports
/// `UserNotifications`; tests, previews, and fixtures use ``FakeNotificationPoster``.
nonisolated protocol NotificationPosting: Sendable {
    /// Reads the permission without asking.
    @concurrent func permission() async -> NotificationPermission
    /// Asks for permission. macOS shows its prompt only the first time; after that it answers
    /// with what the person chose.
    @concurrent func requestPermission() async -> NotificationPermission
    /// Shows one notification. Callers post only with permission.
    @concurrent func post(_ notification: AppNotification) async
    /// Opens the notification settings of Stackly in System Settings.
    func openSystemSettings()
    /// Sets the handler for a click on a notification, including the click that launched the app.
    func installClickHandler(_ handler: @escaping @MainActor @Sendable (NotificationTarget) -> Void)
}
