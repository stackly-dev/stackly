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
    /// The choices that belong to this Mac, such as Appearance (spec 0032).
    public let preferences: AppPreferences
    /// Open Stackly at login, read from the OS (spec 0032).
    public let loginItem: LoginItemModel
    /// Moves the app between `regular` and `accessory` as its windows open and close
    /// (spec 0032, AC-11). The app delegate starts it at launch.
    public let activation: ActivationPolicyController
    /// What the menu bar popover asks the main window to show (spec 0032, AC-3).
    public let destinations: ShellDestinationQueue
    /// The menu bar item and its popover (spec 0032).
    public let menuBar: MenuBarModel
    /// Notifications: permission, the four triggers, and what a click opens (spec 0032).
    public let notifications: NotificationModel
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
    ///   - preferences: The app's own choices; one that applies nothing to the app by default.
    ///   - loginItem: Open Stackly at login; the live `SMAppService.mainApp` by default.
    ///   - notifications: The notification model the shell follows events with, or `nil` for one
    ///     that never posts and follows nothing.
    ///   - activity: Keeps the app out of App Nap while it runs; the process's own by default.
    public init(
        model: DaemonModel, closeNotice: CloseNotice = CloseNotice(),
        commandLineTool: CommandLineToolModel? = nil,
        onboardingStore: any OnboardingRecordStore = UserDefaultsOnboardingRecordStore(),
        preferences: AppPreferences = AppPreferences(apply: { _ in }),
        loginItem: LoginItemModel = LoginItemModel(),
        notifications: NotificationModel? = nil,
        activity: ShellActivity = .process
    ) {
        self.model = model
        self.preferences = preferences
        self.loginItem = loginItem
        activation = ActivationPolicyController()
        destinations = ShellDestinationQueue()
        menuBar = MenuBarModel(daemon: model)
        self.notifications = notifications ?? .inert(preferences: preferences)
        self.closeNotice = closeNotice
        self.commandLineTool = commandLineTool
        onboarding = OnboardingModel(store: onboardingStore)
        self.activity = activity
        // App Nap starves a hidden app under load: it then learns of a lost or new daemon
        // and applies events tens of seconds late, past the waits of spec 0013.
        activityToken = activity.begin()
        model.start()
        if let notifications {
            attach(notifications)
        }
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

    /// Connects `notifications` to the model's events, snapshots, and connection, and to the
    /// main window a click opens.
    private func attach(_ notifications: NotificationModel) {
        let model = self.model
        notifications.attach(
            sync: model.sync, instances: model.instances,
            titleOf: { operation in
                OperationTitle.title(
                    for: operation,
                    instanceNames: Dictionary(
                        model.instances.instances.map { ($0.id, $0.name) },
                        uniquingKeysWith: { first, _ in first }),
                    runtimes: OperationTitle.RuntimeNames(store: model.runtimes))
            },
            isReachable: { model.presentation.hasConnection },
            open: { [weak self] _ in
                // The app is `regular` and active before the window opens (spec 0032, AC-11).
                self?.activation.prepareToShowWindow()
                self?.windowOpener.openMain()
            })
    }

    /// Opens the main window on the Catalog with `provider` selected: Show in Catalog in
    /// Settings › Sources (spec 0037). The window takes the destination when it shows, also
    /// when it opens because of this request.
    public func showInCatalog(_ provider: String) {
        destinations.request(.provider(provider))
        activation.prepareToShowWindow()
        windowOpener.openMain()
    }

    /// The app became active: retry now when not connected, and read the background
    /// agent's status again (spec 0015). Neither changes the registration. The login item and
    /// the notification permission are read again too, since the person may have changed them
    /// in System Settings.
    public func didBecomeActive() {
        model.systemRetry()
        Task { await loginItem.refresh() }
        Task { await notifications.refreshPermission() }
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
        // The windows a login item launch closes are not the person's doing.
        guard activation.isClosingForLaunch == false else { return }
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
            onboardingStore: onboardingStore, notificationPoster: UserNotificationPoster()
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
    ///   - notificationPoster: The macOS side of notifications, or `nil` for a shell that never
    ///     posts (every test). `live` gives ``UserNotificationPoster``.
    static func assemble(
        rootOverride: String?,
        clientVersion: String,
        now: @escaping @Sendable () -> Date = { Date() },

        closeNotice: CloseNotice = CloseNotice(),
        agentController: (any BackgroundAgentControlling)? = nil,
        commandLineTool: (any CommandLineToolControlling)? = nil,
        onboardingStore: any OnboardingRecordStore = UserDefaultsOnboardingRecordStore(),
        notificationPoster: (any NotificationPosting)? = nil
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
        model.startFacts = .live(home: UserHome.current())
        let preferences = AppPreferences()
        return LiveShellParts(
            runtime: ShellRuntime(
                model: model, closeNotice: closeNotice,
                commandLineTool: commandLineTool.map { CommandLineToolModel(controller: $0) },
                onboardingStore: onboardingStore, preferences: preferences,
                notifications: notificationPoster.map {
                    NotificationModel(poster: $0, preferences: preferences)
                }),
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
