import Foundation
import StacklyClient

/// What the Daemon section of Settings says about stacklyd and whether Restart Daemon can
/// run (spec 0030 AC-16): a pure value of the connection, so each row of the mockup can be
/// tested and drawn without a daemon.
nonisolated public struct DaemonSettingsContent: Equatable, Sendable {
    /// The line under "stacklyd".
    public let subtitle: String
    /// Why the button is off, or `nil` while it can run.
    public let disabledReason: String?

    /// The content for a connection.
    /// - Parameters:
    ///   - isConnected: Whether the app holds a connection to a daemon.
    ///   - protocolText: The protocol version the daemon accepted, as the footer writes it.
    ///   - launchContext: Who started the daemon, or `nil` when it did not say.
    ///   - isRestarting: Whether a restart runs now.
    public static func make(
        isConnected: Bool, protocolText: String?, launchContext: LaunchContextKind?,
        isRestarting: Bool
    ) -> DaemonSettingsContent {
        guard isConnected else {
            return DaemonSettingsContent(
                subtitle: RestartCopy.disconnectedSubtitle,
                disabledReason: RestartCopy.disconnectedReason)
        }
        if launchContext == .manual {
            return DaemonSettingsContent(
                subtitle: RestartCopy.manualSubtitle, disabledReason: RestartCopy.manualSubtitle)
        }
        let subtitle = RestartCopy.connectedSubtitle(protocolText: protocolText ?? "—")
        return DaemonSettingsContent(
            subtitle: subtitle,
            disabledReason: isRestarting ? RestartCopy.inProgressReason : nil)
    }
}
