import StacklyUI
import SwiftUI

@main
struct StacklyApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        Window("Stackly", id: WindowOpener.mainWindowID) {
            // provisional: F13 (milestone 4 wires the live connection model; until then the
            // shell shows its first connection attempt)
            let status = ShellStatus.connecting(slow: false, since: appDelegate.launchedAt)
            AppShell(
                presentation: ShellPresentation(status: status),
                diagnostics: ConnectionDiagnostics(status: status, appProtocol: nil),
                windowOpener: appDelegate.windowOpener,
                windowWillClose: { appDelegate.mainWindowWillClose($0) },
                reconnect: {},
                refreshDiagnostics: {}
            )
        }
        .defaultSize(width: 1280, height: 800)
        .windowResizability(.contentMinSize)

        Settings {
            // provisional: F32
            SettingsPlaceholder()
        }

        #if DEBUG
            // The design gallery of spec 0012, listed in the Window menu. Debug builds only.
            Window("Design Gallery", id: "design-gallery") {
                DesignGallery()
            }
            .defaultSize(width: 1100, height: 780)
        #endif
    }
}
