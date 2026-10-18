import AppKit
import SwiftUI

/// Reports when the window that hosts it is about to close.
///
/// ``AppShell`` places it in the background of the main window's content, so the main
/// window is known through this accessor, never through an identifier SwiftUI generates.
struct MainWindowAccessor: NSViewRepresentable {
    /// Called with the window right before it closes.
    let windowWillClose: (NSWindow) -> Void

    func makeNSView(context: Context) -> WindowCloseReportingView {
        let view = WindowCloseReportingView()
        view.windowWillClose = windowWillClose
        return view
    }

    func updateNSView(_ view: WindowCloseReportingView, context: Context) {
        view.windowWillClose = windowWillClose
    }
}

/// An empty view that observes `NSWindow.willCloseNotification` for the window it is in.
final class WindowCloseReportingView: NSView {
    /// Called with the window right before it closes.
    var windowWillClose: ((NSWindow) -> Void)?

    private var observation: (any NSObjectProtocol)?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        stopObserving()
        guard let window else { return }
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
