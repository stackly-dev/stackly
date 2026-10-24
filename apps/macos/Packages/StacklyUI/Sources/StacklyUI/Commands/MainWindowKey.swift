import AppKit
import Observation
import SwiftUI

/// Whether one main window is the key window (spec 0069, AC-4).
/// The view at the root of the window holds one of these, puts it in the environment, and ``MainWindowAccessor`` hands it the window. It listens to the notifications of that window only, so another window becoming key never changes it.
@MainActor @Observable public final class MainWindowKey {
    /// Whether the window is the key window. A sheet, an alert, Settings, or another window
    /// that is key makes it `false`.
    public private(set) var isKey = false

    @ObservationIgnored private weak var window: NSWindow?
    @ObservationIgnored private var observers: [any NSObjectProtocol] = []
    @ObservationIgnored private let isKeyWindow: @MainActor (NSWindow) -> Bool

    /// Creates a state that is not key until a window is tracked.
    public init() {
        isKeyWindow = { $0.isKeyWindow }
    }

    /// Creates a state with its own way to ask a window whether it is key, so a test does
    /// not need a running application.
    init(isKeyWindow: @escaping @MainActor (NSWindow) -> Bool) {
        self.isKeyWindow = isKeyWindow
    }

    isolated deinit {
        stopObserving()
    }

    /// Whether the window is the key window right now. The run guard of a menu item reads
    /// it when the item runs, in case the menu was not recomputed yet.
    public var isKeyNow: Bool {
        guard let window else { return false }
        return isKeyWindow(window)
    }

    /// Starts following `window`. ``MainWindowAccessor`` calls it when the window is known.
    func track(_ window: NSWindow) {
        guard self.window !== window else { return }
        stopObserving()
        self.window = window
        isKey = isKeyWindow(window)
        let center = NotificationCenter.default
        observers = [
            center.addObserver(
                forName: NSWindow.didBecomeKeyNotification, object: window, queue: .main
            ) { [weak self] _ in
                // AppKit posts it on the main thread, and the observer runs on the main queue.
                MainActor.assumeIsolated { self?.isKey = true }
            },
            center.addObserver(
                forName: NSWindow.didResignKeyNotification, object: window, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.isKey = false }
            },
            center.addObserver(
                forName: NSWindow.willCloseNotification, object: window, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.isKey = false }
            },
        ]
    }

    private func stopObserving() {
        for observer in observers {
            NotificationCenter.default.removeObserver(observer)
        }
        observers = []
    }
}

extension EnvironmentValues {
    /// The key state of the window this view is in, or `nil` outside a main window.
    @Entry var mainWindowKey: MainWindowKey? = nil
}
