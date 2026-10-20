import AppKit
import StacklyUI

/// The AppKit hooks of the app (spec 0013, Shell jendela).
///
/// It builds the shell once at launch and keeps it for the whole launch, so closing the
/// main window stops nothing: the connection, the daemon, and every instance keep
/// running. The app stays alive after its last window closes, a Dock click opens the main
/// window again, and Quit is marked before any window closes, so the close notice never
/// shows while quitting. Quit only closes the socket; nothing is sent to the daemon.
/// Checked on macOS 27; see `apps/macos/design.md`, App shell.
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// The shell: the daemon model, the window opener, the close notice, and the
    /// lifecycle hooks. `STACKLY_ROOT` and the bundle version are read once, here.
    let runtime = ShellBootstrap.live(
        rootOverride: ProcessInfo.processInfo.environment["STACKLY_ROOT"],
        clientVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString")
            as? String ?? ""
    )
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
        return runtime.windowOpener.openMain() == false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        isTerminating = true
        return .terminateNow
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Closes the daemon socket synchronously; the link never tries again.
        runtime.willTerminate()
    }

    /// Shows the close notice when the main window closes, unless the app is quitting.
    func mainWindowWillClose(_ window: NSWindow) {
        runtime.mainWindowWillClose(isTerminating: isTerminating, screen: window.screen)
    }
}
