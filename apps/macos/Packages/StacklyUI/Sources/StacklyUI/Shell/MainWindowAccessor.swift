import AppKit
import SwiftUI

/// Reports when the window that hosts it is about to close, and hands the window to the
/// ``MainWindowKey`` that follows its key state.
///
/// ``AppShell`` places it in the background of the main window's content, so the main
/// window is known through this accessor, never through an identifier SwiftUI generates.
struct MainWindowAccessor: NSViewRepresentable {
    /// The key state of the window, which starts following it when it is known.
    let windowKey: MainWindowKey
    /// Called with the window right before it closes.
    let windowWillClose: (NSWindow) -> Void

    func makeNSView(context: Context) -> WindowCloseReportingView {
        let view = WindowCloseReportingView()
        view.windowKey = windowKey
        view.windowWillClose = windowWillClose
        return view
    }

    func updateNSView(_ view: WindowCloseReportingView, context: Context) {
        view.windowKey = windowKey
        view.windowWillClose = windowWillClose
    }
}

/// An empty view that observes `NSWindow.willCloseNotification` for the window it is in.
final class WindowCloseReportingView: NSView {
    /// The key state that follows the window.
    var windowKey: MainWindowKey?
    /// Called with the window right before it closes.
    var windowWillClose: ((NSWindow) -> Void)?

    private var observation: (any NSObjectProtocol)?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        stopObserving()
        guard let window else { return }
        windowKey?.track(window)
        observation = NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification,
            object: window,
            queue: .main
        ) { [weak self] _ in
            // AppKit posts it on the main thread, and the observer runs on the main queue.
            MainActor.assumeIsolated {
                guard let self, let window = self.window else { return }
                self.windowWillClose?(window)
            }
        }
    }

    private func stopObserving() {
        if let observation {
            NotificationCenter.default.removeObserver(observation)
        }
        observation = nil
    }
}
