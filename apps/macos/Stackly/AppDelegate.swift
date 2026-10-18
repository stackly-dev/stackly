import AppKit
import StacklyUI

/// The AppKit hooks of the app (spec 0013, Shell jendela).
///
/// The app stays alive after its last window closes, a Dock click opens the main window
/// again, and Quit is marked before any window closes, so the close notice never shows
/// while quitting. Checked on macOS 27; see `apps/macos/design.md`, App shell.
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// When the app launched: the start of the first connection attempt.
    let launchedAt = Date()
    /// Opens the main window again; `AppShell` captures its action.
    let windowOpener = WindowOpener()
    /// The notice shown the first time the main window closes in this launch.
    let closeNotice = CloseNotice()
    /// Whether the app is quitting. Set before any window closes.
    private(set) var isTerminating = false

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool)
        -> Bool
    {
        // A minimized window counts as visible, and AppKit restores it by itself.
        guard flag == false else { return true }
        return windowOpener.openMain() == false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        isTerminating = true
        return .terminateNow
    }

    func applicationWillTerminate(_ notification: Notification) {
        // provisional: F13 (milestone 4 calls ShellRuntime.willTerminate() here, which
        // closes the daemon socket synchronously)
    }

    /// Shows the close notice when the main window closes, unless the app is quitting.
    func mainWindowWillClose(_ window: NSWindow) {
        closeNotice.mainWindowWillClose(isTerminating: isTerminating, screen: window.screen)
    }
}
