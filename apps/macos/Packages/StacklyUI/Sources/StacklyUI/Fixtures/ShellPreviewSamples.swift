#if DEBUG
    import StacklyClient
    import SwiftUI

    /// Sample presentations for the shell previews.
    enum ShellPreviewSamples {
        static let now = FixtureClock.now

        static let unreachable = ShellPresentation(
            status: .unreachable(lastKnownAt: now.addingTimeInterval(-300), since: now),
            lastErrorCode: "ipc.endpoint_missing"
        )

        static let diagnostics = ConnectionDiagnostics(
            status: .unreachable(lastKnownAt: now.addingTimeInterval(-300), since: now),
            socketPath:
                "/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/stackly-1a2b3c4d/stacklyd.sock",
            lastError: DiagnosticsError(
                code: "ipc.endpoint_missing",
                category: "unavailable",
                message: "no socket at the daemon path"
            ),
            nextAttempt: now.addingTimeInterval(5),
            lastConnectedAt: now.addingTimeInterval(-300),
            daemon: DaemonFacts(
                version: "0.1.0",
                pid: 4242,
                startedAt: now.addingTimeInterval(-7_200),
                serverInstanceId: "0192f4a1-7c1e-7b3a-9d2e-5f6a7b8c9d0e"
            ),
            daemonProtocol: "1.0",
            appProtocol: "1.0",
            contractDrift: false
        )

        static func shell(_ presentation: ShellPresentation) -> some View {
            AppShell(
                presentation: presentation,
                diagnostics: diagnostics,
                counts: SidebarCounts(instances: 6, runningOperations: 2),
                windowOpener: WindowOpener(),
                windowWillClose: { _ in },
                reconnect: {},
                refreshDiagnostics: {}
            )
        }
    }
#endif
