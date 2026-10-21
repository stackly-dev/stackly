import SwiftUI

extension AppShell {
    /// The View > section commands in sidebar order, so ⌘1 to ⌘7 follow the sidebar.
    static let sectionCommands: [(command: CommandID, section: SidebarSection)] = [
        (.showOverview, .overview), (.showInstances, .instances),
        (.showEnvironments, .environments), (.showCatalog, .catalog),
        (.showRuntimes, .runtimes), (.showOperations, .operations),
        (.showDiagnostics, .diagnostics),
    ]

    /// The actions the shell publishes with `.publishesCommands(.shell, …)` (spec 0069,
    /// *Penerbit*).
    ///
    /// The section commands are always there and never change data. Reconnect is there only
    /// while there is no connection: busy while an attempt runs, and disabled with the same
    /// reason as the Diagnostics button while the app is misconfigured.
    ///
    /// - Parameters:
    ///   - presentation: What the shell shows for the daemon's state.
    ///   - select: Shows a section, the same function the sidebar uses.
    ///   - reconnect: Tries to connect now.
    static func shellActions(
        presentation: ShellPresentation,
        select: @escaping @MainActor (SidebarSection) -> Void,
        reconnect: @escaping @MainActor () -> Void
    ) -> [CommandID: StacklyAction] {
        var actions: [CommandID: StacklyAction] = [:]
        for (command, section) in sectionCommands {
            actions[command] = StacklyAction(
                id: command.rawValue, title: CommandMap.entry(for: command).title,
                mutates: false, perform: { select(section) })
        }
        if presentation.hasConnection == false {
            actions[.reconnect] = StacklyAction(
                id: CommandID.reconnect.rawValue,
                title: CommandMap.entry(for: .reconnect).title, mutates: false,
                disabledReason: presentation.isMisconfigured
                    ? ConnectionDiagnosticsSection.reconnectDisabledReason : nil,
                isBusy: presentation.isReconnecting, perform: reconnect)
        }
        return actions
    }
}
