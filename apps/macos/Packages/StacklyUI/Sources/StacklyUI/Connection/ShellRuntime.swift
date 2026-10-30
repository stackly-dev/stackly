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
    /// The command line tool, or `nil` for a shell without one (spec 0015).
    public let commandLineTool: CommandLineToolModel?
    /// Whether the onboarding shows in the main window and at which step (spec 0016).
    public let onboarding: OnboardingModel
    private var observers: [any NSObjectProtocol] = []
    private let activity: ShellActivity
    private var activityToken: (any NSObjectProtocol)?

    /// Wraps `model`, starts it, and listens for the app becoming active and
    /// the Mac waking, which retry a lost connection.
    /// - Parameters:
    ///   - model: The daemon model.
    ///   - closeNotice: The notice shown the first time the main window closes.
    ///   - commandLineTool: The command line tool model, or `nil`.
    ///   - onboardingStore: Where the finished record of the onboarding lives; the app's
    ///     `UserDefaults` by default.
    ///   - activity: Keeps the app out of App Nap while it runs; the process's own by default.
    public init(
        model: DaemonModel, closeNotice: CloseNotice = CloseNotice(),
        commandLineTool: CommandLineToolModel? = nil,
        onboardingStore: any OnboardingRecordStore = UserDefaultsOnboardingRecordStore(),
        activity: ShellActivity = .process
    ) {
        self.model = model
        self.closeNotice = closeNotice
        self.commandLineTool = commandLineTool
        onboarding = OnboardingModel(store: onboardingStore)
        self.activity = activity
        // App Nap starves a hidden app under load: it then learns of a lost or new daemon
        // and applies events tens of seconds late, past the waits of spec 0013.
        activityToken = activity.begin()
        model.start()
        observers.append(
            NotificationCenter.default.addObserver(
                forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.didBecomeActive() }
            })
        observers.append(
            NotificationCenter.default.addObserver(
                forName: NSApplication.didResignActiveNotification, object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.didResignActive() }
            })
        observers.append(
            NSWorkspace.shared.notificationCenter.addObserver(
                forName: NSWorkspace.didWakeNotification, object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.didWake() }
            })
    }

    /// The app became active: retry now when not connected, and read the background
    /// agent's status again (spec 0015). Neither changes the registration.
    public func didBecomeActive() {
        model.systemRetry()
        if let agent = model.agent {
            agent.resumePolling()
            Task { await agent.refresh() }
        }
        if let commandLineTool {
            Task { await commandLineTool.refresh() }
        }
    }

    /// The app became inactive: the agent's polling pauses, so the status is not read
    /// while the user is in System Settings.
    public func didResignActive() {
        model.agent?.pausePolling()
    }

    /// The Mac woke from sleep: retry now when not connected.
    public func didWake() {
        model.systemRetry()
    }

    /// The app terminates: close the socket synchronously. Nothing is sent to
    /// the daemon.
    public func willTerminate() {
        model.stopNow()
        if let activityToken {
            activity.end(activityToken)
            self.activityToken = nil
        }
    }

    /// The main window is about to close: show the notice the first time,
    /// unless the app quits.
    public func mainWindowWillClose(isTerminating: Bool, screen: NSScreen?) {
        closeNotice.mainWindowWillClose(isTerminating: isTerminating, screen: screen)
    }
}

/// The activity that keeps the shell out of App Nap, so the link, the heartbeat, and the
/// events keep their pace while the window is hidden. It allows idle system sleep.
nonisolated public struct ShellActivity: Sendable {
    /// The options the shell's activity uses.
    public static let options: ProcessInfo.ActivityOptions = .userInitiatedAllowingIdleSystemSleep

    /// Starts the activity and returns its token.
    public var begin: @Sendable () -> any NSObjectProtocol
    /// Ends the activity of a token from ``begin``.
    public var end: @Sendable (any NSObjectProtocol) -> Void

    /// Creates an activity from closures.
    public init(
        begin: @escaping @Sendable () -> any NSObjectProtocol,
        end: @escaping @Sendable (any NSObjectProtocol) -> Void
    ) {
        self.begin = begin
        self.end = end
    }

    /// The process's activity through `ProcessInfo`.
    public static let process = ShellActivity(
        begin: {
            ProcessInfo.processInfo.beginActivity(
                options: options, reason: "Stackly follows stacklyd and its instances")
        },
        end: { ProcessInfo.processInfo.endActivity($0) })
}

/// The one door from the app target into the shell.
public enum ShellBootstrap {
    /// Builds the live shell: the socket path from `rootOverride` (the
    /// `STACKLY_ROOT` the app read once at launch), the link, the sync engine,
    /// and the model.
    /// - Parameters:
    ///   - rootOverride: `STACKLY_ROOT`, or `nil` when it is not set.
    ///   - clientVersion: The bundle's `CFBundleShortVersionString`.
    ///   - now: The wall clock.
    ///   - onboardingStore: Where the finished record of the onboarding lives; the app's
    ///     `UserDefaults` by default.
    public static func live(
        rootOverride: String?,
        clientVersion: String,
        now: @escaping @Sendable () -> Date = { Date() },
        onboardingStore: any OnboardingRecordStore = UserDefaultsOnboardingRecordStore()
    ) -> ShellRuntime {
        assemble(
            rootOverride: rootOverride, clientVersion: clientVersion, now: now,
            agentController: SMAppServiceAgent(), commandLineTool: CommandLineTool.live(),
            onboardingStore: onboardingStore
        ).runtime
    }

    /// Builds the live shell and returns its link and sync engine too, for the end to
    /// end tests that stop them and read the cursor.
    /// - Parameters:
    ///   - rootOverride: `STACKLY_ROOT`, or `nil` when it is not set.
    ///   - clientVersion: The bundle's `CFBundleShortVersionString`.
    ///   - now: The wall clock.
    ///   - closeNotice: The notice shown the first time the main window closes.
    ///   - agentController: The OS side of the background agent, or `nil` for a shell     without an agent (the banner then is the one of spec 0013). `live` gives     ``SMAppServiceAgent``.
    ///   - commandLineTool: The command line tool, or `nil` for a shell without one. `live`     gives ``CommandLineTool``.
    ///   - onboardingStore: Where the finished record of the onboarding lives.
    static func assemble(
        rootOverride: String?,
        clientVersion: String,
        now: @escaping @Sendable () -> Date = { Date() },

        closeNotice: CloseNotice = CloseNotice(),
        agentController: (any BackgroundAgentControlling)? = nil,
        commandLineTool: (any CommandLineToolControlling)? = nil,
        onboardingStore: any OnboardingRecordStore = UserDefaultsOnboardingRecordStore()
    ) -> LiveShellParts {
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
        let rootOverrideActive = rootOverride != nil
        let agent = agentController.map {
            BackgroundAgentModel(
                controller: $0, launchRecord: LaunchRecordReader(home: UserHome.current()),
                rootOverrideActive: rootOverrideActive)
        }
        let model = DaemonModel(
            link: .live(link),
            signals: engine.signals,
            endpoint: try? endpoint.get(),
            launchedAt: now(),
            now: now,

            agent: agent)
        model.rootOverrideActive = rootOverrideActive
        return LiveShellParts(
            runtime: ShellRuntime(
                model: model, closeNotice: closeNotice,
                commandLineTool: commandLineTool.map { CommandLineToolModel(controller: $0) },
                onboardingStore: onboardingStore),
            link: link,
            engine: engine)
    }
}

/// The pieces ``ShellBootstrap`` wires together.
struct LiveShellParts {
    /// What the app target holds.
    let runtime: ShellRuntime
    /// The daemon link under the model.
    let link: DaemonLink
    /// The sync engine that feeds the model's stores.
    let engine: SyncEngine
}
