import StacklyUI
import SwiftUI

@main
struct StacklyApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        Window("Stackly", id: WindowOpener.mainWindowID) {
            LiveShell(
                runtime: appDelegate.runtime,
                windowWillClose: { appDelegate.mainWindowWillClose($0) }
            )
        }
        .defaultSize(width: 1280, height: 800)
        .windowResizability(.contentMinSize)
        // The one menu bar of the app (spec 0069); the shortcut comes from the command map.
        .commands { StacklyCommands(runtime: appDelegate.runtime) }
        .keyboardShortcut(CommandMap.entry(for: .showMainWindow).keyboardShortcut)

        Settings {
            // provisional: F32
            SettingsPlaceholder()
        }

        #if DEBUG
            // The design gallery of spec 0012, listed in the Window menu. Debug builds only.
            Window("Design Gallery", id: "design-gallery") {
                DesignGallery(secrets: appDelegate.runtime.model.secrets)
            }
            .defaultSize(width: 1100, height: 780)
        #endif
    }
}
