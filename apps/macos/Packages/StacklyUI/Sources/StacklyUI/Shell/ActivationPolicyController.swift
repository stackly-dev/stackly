import AppKit
import SwiftUI

/// Keeps the Dock icon and the app menu only while a window of the app is open (spec 0032,
/// AC-11). It counts the app's windows (visible, minimized, or in another Space) from the
/// `NSWindow` notifications; when the last one closes the app becomes `accessory`, and every
/// path that opens a window calls ``prepareToShowWindow()`` first, which makes it `regular`
/// again and active. Nothing here talks to the daemon: closing a window or changing the
/// policy stops nothing.
@MainActor
public final class ActivationPolicyController {
    /// The two policies the app moves between.
    public enum Policy: Equatable, Sendable {
        /// Dock icon and app menu.
        case regular
        /// The menu bar item only.
        case accessory
    }

    /// The side effects on the application, so a test needs no running app.
    struct Host {
        /// Sets the activation policy.
        var setPolicy: @MainActor (Policy) -> Void
        /// Makes the app the active one.
        var activate: @MainActor () -> Void

        /// `NSApp`.
        static let application = Host(
            setPolicy: { policy in
                NSApp.setActivationPolicy(policy == .regular ? .regular : .accessory)
            },
            activate: { NSApp.activate() })
    }

    /// The policy the controller last set.
    public private(set) var policy: Policy = .regular
    /// How many windows of the app are open now.
    public var openWindowCount: Int { open.count }
    /// Whether the main window is open (visible or minimized).
    public var isMainWindowOpen: Bool {
        guard let mainWindow else { return false }
        return open.contains(ObjectIdentifier(mainWindow))
    }
    /// True while ``launchedAsLoginItem()`` closes the windows SwiftUI opened at launch, so
    /// the close notice does not show for them.
    public private(set) var isClosingForLaunch = false

    /// How long after a login item launch a window that opens by itself is closed again.
    static let launchGrace: TimeInterval = 3

    private var open: Set<ObjectIdentifier> = []
    private weak var mainWindow: NSWindow?
    private var suppressUntil: Date?
    private let host: Host
    private let now: () -> Date
    private var observers: [any NSObjectProtocol] = []

    /// Creates a controller over the running application.
    public convenience init() {
        self.init(host: .application)
    }

    init(host: Host, now: @escaping () -> Date = { Date() }) {
        self.host = host
        self.now = now
    }

    isolated deinit {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
    }

    /// Starts listening to the windows of the app. Call it once, at launch.
    public func start() {
        guard observers.isEmpty else { return }
        let center = NotificationCenter.default
        observers = [
            center.addObserver(
                forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main
            ) { [weak self] note in
                // AppKit posts it on the main thread, and the observer runs on the main queue.
                nonisolated(unsafe) let window = note.object as? NSWindow
                MainActor.assumeIsolated {
                    guard let window else { return }
                    self?.windowBecameKey(window)
                }
            },
            center.addObserver(
                forName: NSWindow.willCloseNotification, object: nil, queue: .main
            ) { [weak self] note in
                // AppKit posts it on the main thread, and the observer runs on the main queue.
                nonisolated(unsafe) let window = note.object as? NSWindow
                MainActor.assumeIsolated {
                    guard let window else { return }
                    self?.windowWillClose(window)
                }
            },
        ]
    }

    /// Whether `window` is a window of the app that keeps it `regular`: a titled, normal
    /// level window. The menu bar popover, the close notice, and sheets are panels and do
    /// not count.
    static func counts(_ window: NSWindow) -> Bool {
        window is NSPanel == false && window.styleMask.contains(.titled)
            && window.level == .normal
    }

    /// The main window is known (``MainWindowRoot`` hands it over).
    func trackMainWindow(_ window: NSWindow) {
        mainWindow = window
    }

    private func windowBecameKey(_ window: NSWindow) {
        guard Self.counts(window) else { return }
        if let until = suppressUntil, now() < until {
            // SwiftUI opened it by itself after a login item launch.
            close(window)
            return
        }
        windowOpened(ObjectIdentifier(window))
    }

    private func close(_ window: NSWindow) {
        isClosingForLaunch = true
        window.close()
        isClosingForLaunch = false
    }

    private func windowWillClose(_ window: NSWindow) {
        guard Self.counts(window) else { return }
        windowClosed(ObjectIdentifier(window))
    }

    /// A window of the app is open.
    func windowOpened(_ id: ObjectIdentifier) {
        open.insert(id)
        apply()
    }

    /// A window of the app closed. The last one makes the app `accessory`.
    func windowClosed(_ id: ObjectIdentifier) {
        open.remove(id)
        apply()
    }

    private func apply() {
        let wanted: Policy = open.isEmpty ? .accessory : .regular
        guard wanted != policy else { return }
        policy = wanted
        host.setPolicy(wanted)
    }

    /// Call it before any path opens a window (the popover, a notification, a reopen): the
    /// app becomes `regular` and active, so the window comes to the front with its menu.
    public func prepareToShowWindow() {
        suppressUntil = nil
        if policy != .regular {
            policy = .regular
            host.setPolicy(.regular)
        }
        host.activate()
    }

    /// The app was started as a login item (spec 0032, AC-12): it lives in the menu bar
    /// with no window. The windows SwiftUI already opened close without the close notice.
    public func launchedAsLoginItem() {
        suppressUntil = now().addingTimeInterval(Self.launchGrace)
        for window in NSApp.windows where Self.counts(window) {
            close(window)
        }
        open.removeAll()
        policy = .accessory
        host.setPolicy(.accessory)
    }

    /// Closes the main window, when it is open (Close Window in the popover).
    public func closeMainWindow() {
        mainWindow?.close()
    }
}

/// How the app was launched (spec 0032, AC-12).
nonisolated public enum LaunchOrigin {
    /// Whether `event` is the open application event of a launch as a login item: it carries
    /// `keyAELaunchedAsLogInItem` in its property data. Any other event, or none, is a
    /// normal launch, and the main window opens as usual.
    /// - Parameter event: `NSAppleEventManager.shared().currentAppleEvent` at launch.
    public static func isLoginItem(_ event: NSAppleEventDescriptor?) -> Bool {
        guard let event, event.eventClass == AEEventClass(kCoreEventClass),
            event.eventID == AEEventID(kAEOpenApplication)
        else { return false }
        return event.paramDescriptor(forKeyword: AEKeyword(keyAEPropData))?.enumCodeValue
            == OSType(keyAELaunchedAsLogInItem)
    }
}

/// What the menu bar popover asks the main window to show, until a window takes it
/// (spec 0032, AC-3): the window shows the destination when it exists, also when it opens
/// because of this very request.
@MainActor
@Observable
public final class ShellDestinationQueue {
    /// The destination waiting for a window.
    public private(set) var pending: ShellDestination?

    /// Creates an empty queue.
    public init() {}

    /// Asks for `destination`.
    public func request(_ destination: ShellDestination) {
        pending = destination
    }

    /// A window showed the destination.
    public func take() -> ShellDestination? {
        defer { pending = nil }
        return pending
    }
}

extension EnvironmentValues {
    /// The queue the popover leaves destinations in, or `nil` where the shell has none.
    @Entry public var shellDestinationQueue: ShellDestinationQueue? = nil
}
