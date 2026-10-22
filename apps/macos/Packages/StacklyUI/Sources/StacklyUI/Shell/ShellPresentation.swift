import Foundation

/// The health of a connected daemon, as the shell's footer shows it.
nonisolated public enum DaemonHealthLevel: Hashable, Sendable {
    /// The daemon reports `ready`.
    case ready
    /// The daemon reports `degraded`. Details are in Diagnostics.
    case degraded
    /// The daemon reports a health value this app does not know.
    case unknown
}

/// The daemon's state as the shell shows it: one case for each row of the daemon status
/// table of spec 0013 ("Tampilan status daemon"), with the values that row's copy needs.
///
/// The connection model maps its presence to this enum and builds a ``ShellPresentation``
/// from it. Every value here comes from the daemon or the connection, never from the UI.
nonisolated public indirect enum ShellStatus: Hashable, Sendable {
    /// Connected, every store refreshed, with the daemon's health.
    case connected(DaemonHealthLevel)
    /// Connected, and the stores are loading their snapshots after the state in `after`.
    /// The footer says "Refreshing…" while the banner and the gate of `after` stay.
    case refreshing(after: ShellStatus)
    /// Connected while the daemon reports `starting`, since the handshake.
    case starting(since: Date)
    /// Connected while the daemon is shutting down.
    case stopping(since: Date)
    /// The first attempt after launch. `slow` is true after one second.
    case connecting(slow: Bool, since: Date)
    /// The daemon cannot be reached, since `since`. `lastKnownAt` is when the last frame
    /// arrived after a finished snapshot, or `nil` when no state ever loaded.
    case unreachable(lastKnownAt: Date?, since: Date)
    /// The daemon and this app speak protocols that do not match.
    case incompatible(Incompatibility, since: Date)
    /// The socket or the daemon failed a security check; nothing was sent.
    case untrusted(UntrustedCause, since: Date)
    /// The daemon started in registry recovery mode.
    case recovery(since: Date)
    /// The app cannot connect because of its own configuration. It never retries.
    case misconfigured(Misconfiguration, since: Date)

    /// Why the daemon and this app cannot talk.
    nonisolated public enum Incompatibility: Hashable, Sendable {
        /// `protocol.major_mismatch`: the two protocol versions in `M.m` form.
        case majorMismatch(daemonProtocol: String, appProtocol: String)
        /// The daemon rejected the handshake with a `request.*` error code.
        case handshakeRejected(code: String)
    }

    /// Why the socket was not trusted.
    nonisolated public enum UntrustedCause: Hashable, Sendable {
        /// `ipc.endpoint_untrusted` with its `details.reason`.
        case endpoint(reason: String)
        /// `ipc.peer_rejected`: the daemon rejected this app's user.
        case peerRejected
    }

    /// What in the app's configuration stops it from connecting.
    nonisolated public enum Misconfiguration: CaseIterable, Hashable, Sendable {
        /// `STACKLY_ROOT` is set but empty.
        case rootEmpty
        /// `STACKLY_ROOT` is not an absolute path.
        case rootNotAbsolute
        /// The folder in `STACKLY_ROOT` does not exist.
        case rootMissing
        /// The folder in `STACKLY_ROOT` cannot be read.
        case rootUnreadable
        /// The socket path under `STACKLY_ROOT` would be too long.
        case rootTooLong
        /// macOS did not report the user's temporary folder.
        case tempDirUnavailable
        /// The bundle version cannot be sent in the handshake.
        case invalidClientVersion

        /// The banner title, also the reason mutations are blocked.
        public var title: String {
            switch self {
            case .rootEmpty, .rootNotAbsolute, .rootMissing, .rootUnreadable, .rootTooLong:
                "STACKLY_ROOT is invalid."
            case .tempDirUnavailable:
                "Stackly can’t find its socket folder."
            case .invalidClientVersion:
                "This build of Stackly is invalid."
            }
        }

        /// The banner message, also the Diagnostics hint. It never shows the variable's
        /// value.
        public var message: String {
            switch self {
            case .rootEmpty:
                "STACKLY_ROOT is set but empty. Fix the variable and relaunch Stackly."
            case .rootNotAbsolute:
                "STACKLY_ROOT must be an absolute path. Fix the variable and relaunch Stackly."
            case .rootMissing:
                "The folder in STACKLY_ROOT doesn’t exist. Fix the variable and relaunch Stackly."
            case .rootUnreadable:
                "Stackly couldn’t read the folder in STACKLY_ROOT. Fix the variable and relaunch Stackly."
            case .rootTooLong:
                "The socket path for STACKLY_ROOT would be too long. Use a shorter folder and relaunch Stackly."
            case .tempDirUnavailable:
                "macOS didn’t report a temporary folder for your user. Restart your Mac and try again."
            case .invalidClientVersion:
                "Its version number can’t be sent to stacklyd. Install a release build."
            }
        }
    }
}

/// An action the daemon banner offers. None changes data, so the mutation gate never
/// disables them.
nonisolated public enum DaemonBannerAction: Hashable, Sendable {
    /// Tries to connect now.
    case reconnect
    /// Opens the Diagnostics page.
    case openDiagnostics
    /// Registers the background agent (spec 0015).
    case turnOn
    /// Opens Login Items in System Settings.
    case openLoginItems
    /// Reads the background agent's status again.
    case checkAgain
    /// Starts stacklyd through the background agent.
    case startDaemon

    /// The button title.
    public var title: String {
        switch self {
        case .reconnect: "Reconnect"
        case .openDiagnostics: "Open Diagnostics"
        case .turnOn: "Turn On"
        case .openLoginItems: "Open Login Items"
        case .checkAgain: "Check Again"
        case .startDaemon: "Start stacklyd"
        }
    }
}

/// The one banner the shell shows for the daemon's state, above every page.
nonisolated public struct DaemonBannerContent: Hashable, Sendable {
    /// The message, which may name the time of the last known state.
    nonisolated enum Message: Hashable, Sendable {
        /// A fixed message.
        case text(String)
        /// "Showing the last known state from <time>." followed by `then`.
        case lastKnownState(since: Date, then: String)
    }

    /// The tone: informative, caution, or critical.
    public let tone: Tone
    /// The title, which VoiceOver uses as the banner's label.
    public let title: String
    /// The actions, in order.
    public let actions: [DaemonBannerAction]
    let messageContent: Message

    init(tone: Tone, title: String, message: Message, actions: [DaemonBannerAction] = []) {
        self.tone = tone
        self.title = title
        self.messageContent = message
        self.actions = actions
    }

    /// The message. The time of the last known state uses
    /// ``StaleNote/formattedSince(now:calendar:locale:timeZone:)``.
    ///
    /// Views pass the system values; tests pass fixed ones.
    ///
    /// - Parameters:
    ///   - now: The current time.
    ///   - calendar: The calendar that decides what today is.
    ///   - locale: The locale of the time.
    ///   - timeZone: The time zone of the time.
    public func message(
        now: Date,
        calendar: Calendar,
        locale: Locale,
        timeZone: TimeZone
    ) -> String {
        switch messageContent {
        case .text(let text):
            return text
        case .lastKnownState(let since, let then):
            // Only `since` matters to the format; the reason is never shown here.
            let note = StaleNote(since: since, reason: .outdated(source: "stacklyd"))
            let time = note.formattedSince(
                now: now,
                calendar: calendar,
                locale: locale,
                timeZone: timeZone
            )
            return "Showing the last known state from \(time). \(then)"
        }
    }
}

/// What the shell shows for a ``ShellStatus``: the footer status, the window banner, the
/// mutation gate, and the placeholder for pages that have no data yet.
///
/// It is a pure value built from the daemon status table of spec 0013, so every row can
/// be tested without a daemon.
nonisolated public struct ShellPresentation: Hashable, Sendable {
    /// The status the presentation is built from.
    public let status: ShellStatus
    /// The footer status in the sidebar.
    public let footer: StatusAppearance
    /// The window banner, or `nil` when there is none.
    public let banner: DaemonBannerContent?
    /// Whether actions that change data can run.
    public let mutationAvailability: MutationAvailability
    /// What a page that has no data yet shows instead of loading: "Waiting for stacklyd",
    /// or `nil` while the page should keep loading.
    public let pageFailure: FailureContent?
    /// Whether a connection attempt runs now. The Reconnect buttons show a small spinner.
    public let isReconnecting: Bool

    /// Builds the presentation of a status.
    ///
    /// - Parameters:
    ///   - status: The daemon's state.
    ///   - lastErrorCode: The code of the last connection error, shown in the page
    ///     placeholder, or `nil`.
    ///   - isReconnecting: Whether a connection attempt runs now.
    public init(status: ShellStatus, lastErrorCode: String? = nil, isReconnecting: Bool = false) {
        self.status = status
        self.isReconnecting = isReconnecting
        let row = Self.row(for: status)
        footer = StatusAppearance(
            label: row.footer, tone: row.footerTone, symbol: row.footerTone.symbol)
        banner = row.banner
        mutationAvailability = row.gate
        pageFailure =
            row.pageWaits
            ? FailureContent(
                title: "Waiting for stacklyd",
                message: "Stackly shows this page once the daemon is reachable.",
                code: lastErrorCode
            )
            : nil
    }

    /// Whether the daemon is connected and every store refreshed. Only then does the
    /// Operations badge show, and the Instances badge is not "last known".
    public var isConnected: Bool {
        if case .connected = status { return true }
        return false
    }

    /// Whether a connection to the daemon is open, refreshed or not: Diagnostics offers
    /// Refresh instead of Reconnect.
    public var hasConnection: Bool {
        switch status {
        case .connected, .refreshing, .starting, .stopping, .recovery: true
        case .connecting, .unreachable, .incompatible, .untrusted, .misconfigured: false
        }
    }

    /// Whether the app cannot connect until it is relaunched.
    public var isMisconfigured: Bool {
        if case .misconfigured = status { return true }
        return false
    }

    /// One row of the daemon status table.
    private struct Row {
        var footer: String
        var footerTone: Tone
        var banner: DaemonBannerContent?
        var gate: MutationAvailability
        /// Whether pages without data show "Waiting for stacklyd": every row but
        /// connected, refreshing, starting, and the first attempt.
        var pageWaits: Bool
    }

    private static let unavailableMessage = "Actions are disabled until the daemon is reachable."
    private static let incompatibleTitle = "This version of Stackly can’t talk to stacklyd."

    private static func row(for status: ShellStatus) -> Row {
        switch status {
        case .connected(let health):
            connectedRow(health)
        case .refreshing(let previous):
            refreshingRow(after: previous)
        case .starting(let since):
            Row(
                footer: "stacklyd starting…",
                footerTone: .caution,
                banner: DaemonBannerContent(
                    tone: .informative,
                    title: "stacklyd is starting.",
                    message: .text("Actions are available once it’s ready.")
                ),
                gate: .blocked(reason: "stacklyd is starting.", since: since),
                pageWaits: false
            )
        case .stopping(let since):
            Row(
                footer: "stacklyd stopping…",
                footerTone: .caution,
                banner: DaemonBannerContent(
                    tone: .informative,
                    title: "stacklyd is shutting down.",
                    message: .text("Stackly reconnects when it’s back."),
                    actions: [.openDiagnostics]
                ),
                gate: .blocked(reason: "stacklyd is shutting down.", since: since),
                pageWaits: true
            )
        case .connecting(let slow, let since):
            Row(
                footer: "Connecting…",
                footerTone: .caution,
                banner: slow
                    ? DaemonBannerContent(
                        tone: .informative,
                        title: "Connecting to stacklyd…",
                        message: .text("Actions are available once it’s connected."),
                        actions: [.openDiagnostics]
                    ) : nil,
                gate: .blocked(reason: "Connecting to stacklyd…", since: since),
                pageWaits: false
            )
        case .unreachable(let lastKnownAt, let since):
            Row(
                footer: "stacklyd unavailable",
                footerTone: .critical,
                banner: DaemonBannerContent(
                    tone: .caution,
                    title: "Can’t connect to stacklyd.",
                    message: lastKnownAt.map {
                        .lastKnownState(since: $0, then: unavailableMessage)
                    } ?? .text(unavailableMessage),
                    actions: [.reconnect, .openDiagnostics]
                ),
                gate: .blocked(reason: "stacklyd is unavailable.", since: since),
                pageWaits: true
            )
        case .incompatible(let incompatibility, let since):
            Row(
                footer: "stacklyd incompatible",
                footerTone: .critical,
                banner: DaemonBannerContent(
                    tone: .critical,
                    title: incompatibleTitle,
                    message: .text(incompatibleMessage(incompatibility)),
                    actions: [.reconnect, .openDiagnostics]
                ),
                gate: .blocked(reason: incompatibleTitle, since: since),
                pageWaits: true
            )
        case .untrusted(let cause, let since):
            Row(
                footer: "stacklyd untrusted",
                footerTone: .critical,
                banner: DaemonBannerContent(
                    tone: .critical,
                    title: "Stackly didn’t connect to stacklyd.",
                    message: .text(
                        "The socket failed a security check: \(ConnectionHint.untrustedReason(cause)). Nothing was sent."
                    ),
                    actions: [.reconnect, .openDiagnostics]
                ),
                gate: .blocked(reason: "stacklyd’s socket failed a security check.", since: since),
                pageWaits: true
            )
        case .recovery(let since):
            Row(
                footer: "stacklyd in recovery",
                footerTone: .critical,
                banner: DaemonBannerContent(
                    tone: .critical,
                    title: "Stackly’s registry needs recovery.",
                    message: .text(
                        "stacklyd started in recovery mode and didn’t change your data. Instances can’t be managed until the registry is repaired."
                    ),
                    actions: [.openDiagnostics]
                ),
                gate: .blocked(reason: "Stackly’s registry needs recovery.", since: since),
                pageWaits: true
            )
        case .misconfigured(let problem, let since):
            Row(
                footer: "Stackly misconfigured",
                footerTone: .critical,
                banner: DaemonBannerContent(
                    tone: .critical,
                    title: problem.title,
                    message: .text(problem.message),
                    actions: [.openDiagnostics]
                ),
                gate: .blocked(reason: problem.title, since: since),
                pageWaits: true
            )
        }
    }

    private static func connectedRow(_ health: DaemonHealthLevel) -> Row {
        let (footer, tone): (String, Tone) =
            switch health {
            case .ready: ("stacklyd running", .positive)
            case .degraded: ("stacklyd degraded", .caution)
            case .unknown: ("stacklyd status unknown", .caution)
            }
        return Row(
            footer: footer, footerTone: tone, banner: nil, gate: .available, pageWaits: false)
    }

    /// Refreshing keeps the banner and the gate of the state before it. After the first
    /// attempt at launch, the gate says the latest state is loading.
    private static func refreshingRow(after previous: ShellStatus) -> Row {
        let before = row(for: previous)
        let gate: MutationAvailability =
            if case .connecting(_, let since) = previous {
                .blocked(reason: "Loading the latest state.", since: since)
            } else {
                before.gate
            }
        return Row(
            footer: "Refreshing…",
            footerTone: .caution,
            banner: before.banner,
            gate: gate,
            pageWaits: false
        )
    }

    private static func incompatibleMessage(_ incompatibility: ShellStatus.Incompatibility)
        -> String
    {
        switch incompatibility {
        case .majorMismatch(let daemonProtocol, let appProtocol):
            "stacklyd speaks protocol \(daemonProtocol); this app speaks \(appProtocol). Install matching versions of Stackly and stacklyd."
        case .handshakeRejected(let code):
            "stacklyd rejected the handshake (\(code)). Install matching versions of Stackly and stacklyd."
        }
    }
}
