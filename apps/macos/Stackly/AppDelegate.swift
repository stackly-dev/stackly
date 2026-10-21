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
    let runtime = AppDelegate.makeRuntime()
    /// Whether the app is quitting. Set before any window closes.
    private(set) var isTerminating = false

    /// Whether the Debug app plays a fixture scenario (spec 0068). Always false in Release.
    static var isPlayingScenario: Bool {
        #if DEBUG
            return ProcessInfo.processInfo.environment["STACKLY_UI_SCENARIO"] != nil
        #else
            return false
        #endif
    }

    /// Builds the shell runtime. A Debug build that finds `STACKLY_UI_SCENARIO` plays that
    /// fixture scenario and never opens a link; an unknown name stops the app before any
    /// link exists (exit 64, `EX_USAGE`). Every other launch is the live shell.
    private static func makeRuntime() -> ShellRuntime {
        #if DEBUG
            if let name = ProcessInfo.processInfo.environment["STACKLY_UI_SCENARIO"] {
                guard let id = FixtureScenario.ID(rawValue: name) else {
                    FileHandle.standardError.write(
                        Data("unknown STACKLY_UI_SCENARIO \(name)\n".utf8))
                    exit(64)
                }
                return ShellBootstrap.fixture(FixtureScenario(id))
            }
        #endif
        return ShellBootstrap.live(
            rootOverride: ProcessInfo.processInfo.environment["STACKLY_ROOT"],
            clientVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString")
                as? String ?? ""
        )
    }

    func applicationWillFinishLaunching(_ notification: Notification) {
        #if DEBUG
            guard let value = ProcessInfo.processInfo.environment["STACKLY_UI_APPEARANCE"] else {
                return
            }
            switch value {
            case "light": NSApp.appearance = NSAppearance(named: .aqua)
            case "dark": NSApp.appearance = NSAppearance(named: .darkAqua)
            default:
                FileHandle.standardError.write(
                    Data("ignoring STACKLY_UI_APPEARANCE \(value)\n".utf8))
            }
        #endif
    }

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
