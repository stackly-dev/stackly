import AppKit
import Foundation
import StacklyClient

/// Everything the app target holds for the shell: the daemon model, the
/// window opener, the close notice, and the app's lifecycle hooks (spec 0013,
/// *Model shell di main actor*).
public final class ShellRuntime {
    /// The daemon model.
    public let model: DaemonModel
    /// Opens the main window again.
    public let windowOpener = WindowOpener()
    /// The notice shown the first time the main window closes.
    public let closeNotice: CloseNotice
    private var observers: [any NSObjectProtocol] = []

    /// Wraps `model`, starts it, and listens for the app becoming active and
    /// the Mac waking, which retry a lost connection.
    public init(model: DaemonModel, closeNotice: CloseNotice = CloseNotice()) {
        self.model = model
        self.closeNotice = closeNotice
        model.start()
        observers.append(
            NotificationCenter.default.addObserver(
                forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.didBecomeActive() }
            })
        observers.append(
            NSWorkspace.shared.notificationCenter.addObserver(
                forName: NSWorkspace.didWakeNotification, object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.didWake() }
            })
    }

    /// The app became active: retry now when not connected.
    public func didBecomeActive() {
        model.systemRetry()
    }

    /// The Mac woke from sleep: retry now when not connected.
    public func didWake() {
        model.systemRetry()
    }

    /// The app terminates: close the socket synchronously. Nothing is sent to
    /// the daemon.
    public func willTerminate() {
        model.stopNow()
    }

    /// The main window is about to close: show the notice the first time,
    /// unless the app quits.
    public func mainWindowWillClose(isTerminating: Bool, screen: NSScreen?) {
        closeNotice.mainWindowWillClose(isTerminating: isTerminating, screen: screen)
    }
}

/// The one door from the app target into the shell.
public enum ShellBootstrap {
    /// Builds the live shell: the socket path from `rootOverride` (the
    /// `STACKLY_ROOT` the app read once at launch), the link, the sync engine,
    /// and the model.
    ///
    /// - Parameters:
    ///   - rootOverride: `STACKLY_ROOT`, or `nil` when it is not set.
    ///   - clientVersion: The bundle's `CFBundleShortVersionString`.
    ///   - now: The wall clock.
    public static func live(
        rootOverride: String?,
        clientVersion: String,
        now: @escaping @Sendable () -> Date = { Date() }
    ) -> ShellRuntime {
        let endpoint = Result { () throws(RootOverrideProblem) -> SocketEndpoint in
            try SocketPathResolver.current(rootOverride: rootOverride)
        }
        let link = DaemonLink(
            endpoint: endpoint,
            clientVersion: clientVersion,
            pid: UInt32(ProcessInfo.processInfo.processIdentifier),
            now: now)
        let engine = SyncEngine(link: link, now: now)
        Task { await engine.start() }
        let model = DaemonModel(
            link: .live(link),
            signals: engine.signals,
            endpoint: try? endpoint.get(),
            launchedAt: now(),
            now: now)
        return ShellRuntime(model: model)
    }
}
