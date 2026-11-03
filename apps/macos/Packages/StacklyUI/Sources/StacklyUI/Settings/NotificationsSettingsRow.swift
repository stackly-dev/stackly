import SwiftUI

/// The Notifications row of the Instances tab (spec 0032 AC-22). The toggle belongs to the app,
/// so it works while stacklyd is offline. Turning it on asks macOS for permission the first
/// time; if the person said no, the toggle stays on and the row says nothing will come, with a
/// button to System Settings. The permission is read again each time the app becomes active.
struct NotificationsSettingsRow: View {
    let notifications: NotificationModel
    let preferences: AppPreferences

    /// What the row says under the toggle when macOS refuses.
    static let deniedText = "Notifications are turned off for Stackly in System Settings."
    /// The note under the title.
    static let note = "Failures, long operations, and lost connection to stacklyd."

    private var isDenied: Bool {
        preferences.notificationsEnabled && notifications.permission == .denied
    }

    var body: some View {
        LabeledContent {
            HStack(spacing: Spacing.s8) {
                if isDenied {
                    ActionButton(
                        StacklyAction(
                            id: "settings.notifications.openSystemSettings",
                            title: "Open Notification Settings", mutates: false,
                            perform: { notifications.openSystemSettings() }),
                        showsReason: false)
                }
                Toggle(
                    "Notifications",
                    isOn: Binding(
                        get: { preferences.notificationsEnabled },
                        set: { on in Task { await notifications.setEnabled(on) } })
                )
                .labelsHidden()
            }
        } label: {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text("Notifications")
                Text(Self.note)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                if isDenied {
                    Text(Self.deniedText)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }
}
