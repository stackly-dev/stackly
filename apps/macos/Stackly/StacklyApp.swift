import StacklyUI
import SwiftUI

#if DEBUG
    import AppKit
#endif

@main
struct StacklyApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        Window("Stackly", id: WindowOpener.mainWindowID) {
            MainWindowRoot(
                runtime: appDelegate.runtime,
                windowWillClose: { appDelegate.mainWindowWillClose($0) }
            )
            #if DEBUG
                .fixtureWindow()
            #endif
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
                    .fixtureWindow()
            }
            .defaultSize(width: 1100, height: 780)
        #endif
    }
}

#if DEBUG
    extension View {
        /// While a fixture scenario plays, fixes the clock and the loading delay and keeps
        /// the window out of state restoration and frame autosave, so the user's own window
        /// size and place are never read or overwritten (spec 0068, AC-12). Without a
        /// scenario it changes nothing.
        @ViewBuilder fileprivate func fixtureWindow() -> some View {
            if AppDelegate.isPlayingScenario {
                self
                    .environment(\.fixtureNow, FixtureClock.now)
                    .environment(\.loadingDelay, .zero)
                    .background(FixtureWindowSettings())
            } else {
                self
            }
        }
    }

    /// Marks the window that hosts it as not restorable and without an autosave name.
    private struct FixtureWindowSettings: NSViewRepresentable {
        func makeNSView(context: Context) -> FixtureWindowSettingsView {
            FixtureWindowSettingsView()
        }

        func updateNSView(_ view: FixtureWindowSettingsView, context: Context) {}
    }

    private final class FixtureWindowSettingsView: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            window?.isRestorable = false
            window?.setFrameAutosaveName("")
        }
    }
#endif
