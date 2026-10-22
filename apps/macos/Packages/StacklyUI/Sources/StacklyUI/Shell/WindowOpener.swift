import SwiftUI

/// Opens the main window again from AppKit code, such as the app delegate when the user
/// clicks Stackly in the Dock after closing the window.
///
/// SwiftUI opens a `Window` scene only through the `openWindow` environment action, so
/// ``MainWindowRoot`` captures that action here when it appears. The captured action keeps
/// working after the main window closed (checked on macOS 27, see `apps/macos/design.md`,
/// App shell).
public final class WindowOpener {
    /// The scene ID of the main window.
    public static let mainWindowID = "main"

    private var openWindow: OpenWindowAction?

    /// Creates an opener with no captured action yet.
    public init() {}

    /// Whether ``MainWindowRoot`` has captured the `openWindow` action yet.
    public var canOpen: Bool {
        openWindow != nil
    }

    /// Keeps the scene's `openWindow` action. ``MainWindowRoot`` calls it when it appears.
    func capture(_ action: OpenWindowAction) {
        openWindow = action
    }

    /// Opens the main window, or brings it to the front when it is open.
    ///
    /// - Returns: `false` when no action was captured yet, so nothing opened.
    @discardableResult
    public func openMain() -> Bool {
        guard let openWindow else { return false }
        openWindow(id: Self.mainWindowID)
        return true
    }
}
