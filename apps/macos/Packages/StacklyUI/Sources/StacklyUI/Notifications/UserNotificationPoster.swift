import AppKit
import Foundation
import Synchronization
import UserNotifications

/// The live ``NotificationPosting``: `UNUserNotificationCenter` (spec 0032, *Notifikasi*). It is
/// the only file that imports `UserNotifications`. The center is read at each call and never
/// stored, so building the poster outside an app bundle (a test) touches nothing.
/// It is also the center's delegate: a click on a notification reaches the handler set with
/// ``installClickHandler(_:)``, and a notification that arrives while Stackly is active is
/// not shown, since the app only posts while it is inactive.
nonisolated final class UserNotificationPoster: NSObject, NotificationPosting,
    UNUserNotificationCenterDelegate
{
    private let handler = Mutex<(@MainActor @Sendable (NotificationTarget) -> Void)?>(nil)

    @concurrent func permission() async -> NotificationPermission {
        Self.map(
            await UNUserNotificationCenter.current().notificationSettings().authorizationStatus)
    }

    @concurrent func requestPermission() async -> NotificationPermission {
        let center = UNUserNotificationCenter.current()
        _ = try? await center.requestAuthorization(options: [.alert])
        return Self.map(await center.notificationSettings().authorizationStatus)
    }

    @concurrent func post(_ notification: AppNotification) async {
        let content = UNMutableNotificationContent()
        content.title = notification.title
        content.body = notification.body
        content.userInfo = notification.target.userInfo
        let request = UNNotificationRequest(
            identifier: notification.identifier, content: content, trigger: nil)
        try? await UNUserNotificationCenter.current().add(request)
    }

    func openSystemSettings() {
        guard
            let url = URL(
                string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension")
        else { return }
        NSWorkspace.shared.open(url)
    }

    func installClickHandler(_ handler: @escaping @MainActor @Sendable (NotificationTarget) -> Void)
    {
        self.handler.withLock { $0 = handler }
        UNUserNotificationCenter.current().delegate = self
    }

    // MARK: - UNUserNotificationCenterDelegate

    func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse
    ) async {
        guard response.actionIdentifier == UNNotificationDefaultActionIdentifier else { return }
        let info = response.notification.request.content.userInfo
        let strings = info.reduce(into: [String: String]()) { result, pair in
            if let key = pair.key as? String, let value = pair.value as? String {
                result[key] = value
            }
        }
        guard let target = NotificationTarget(userInfo: strings),
            let handle = handler.withLock({ $0 })
        else { return }
        await MainActor.run { handle(target) }
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        []
    }

    private static func map(_ status: UNAuthorizationStatus) -> NotificationPermission {
        switch status {
        case .notDetermined: .notDetermined
        case .denied: .denied
        case .authorized, .provisional: .authorized
        @unknown default: .denied
        }
    }
}
