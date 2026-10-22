import Foundation

/// The last connection error, as Diagnostics shows it. The connection model copies it from
/// the daemon's or the client's error body, already redacted.
nonisolated public struct DiagnosticsError: Hashable, Sendable {
    /// The error code, such as `ipc.endpoint_missing`.
    public var code: String
    /// The error category, such as `unavailable`.
    public var category: String
    /// The message, as it came.
    public var message: String
    /// `details.reason`, when there is one.
    public var reason: String?
    /// `details.op` of an `ipc.io_error`.
    public var operation: String?
    /// `details.kind` of an `ipc.io_error`.
    public var kind: String?
    /// `details.method` of an `ipc.call_timeout`, such as `daemon.handshake`.
    public var method: String?
    /// The daemon's own hint (`ErrorBody.hint`), when it sent one.
    public var daemonHint: String?

    /// Creates an error summary.
    public init(
        code: String,
        category: String,
        message: String,
        reason: String? = nil,
        operation: String? = nil,
        kind: String? = nil,
        method: String? = nil,
        daemonHint: String? = nil
    ) {
        self.code = code
        self.category = category
        self.message = message
        self.reason = reason
        self.operation = operation
        self.kind = kind
        self.method = method
        self.daemonHint = daemonHint
    }
}

/// What the daemon last reported about itself (`daemon.get_info`).
nonisolated public struct DaemonFacts: Hashable, Sendable {
    /// The daemon's version.
    public var version: String
    /// The daemon's process ID.
    public var pid: Int
    /// When the daemon started.
    public var startedAt: Date
    /// The daemon's server instance ID.
    public var serverInstanceId: String

    /// Creates the facts.
    public init(version: String, pid: Int, startedAt: Date, serverInstanceId: String) {
        self.version = version
        self.pid = pid
        self.startedAt = startedAt
        self.serverInstanceId = serverInstanceId
    }
}

/// The daemon's health from the last heartbeat (`daemon.get_health`).
nonisolated public struct HealthFacts: Hashable, Sendable {
    /// A subsystem whose status is not `ok`.
    nonisolated public struct Subsystem: Hashable, Sendable {
        /// The subsystem's name.
        public var name: String
        /// The subsystem's status.
        public var status: String
        /// The daemon's detail, when it sent one.
        public var detail: String?

        /// Creates a subsystem entry.
        public init(name: String, status: String, detail: String? = nil) {
            self.name = name
            self.status = status
            self.detail = detail
        }
    }

    /// The overall status, such as `degraded`.
    public var status: String
    /// The subsystems that are not `ok`.
    public var subsystems: [Subsystem]

    /// Creates the health facts.
    public init(status: String, subsystems: [Subsystem] = []) {
        self.status = status
        self.subsystems = subsystems
    }
}

/// One row of the Connection section: a label and a value whose first part may be code.
nonisolated public struct DiagnosticsRow: Hashable, Sendable, Identifiable {
    /// The label, such as "Socket".
    public let label: String
    /// The part shown monospaced first, such as an error code or the socket path.
    public let code: String?
    /// The plain part, after the code.
    public let text: String?

    /// The row's identity: its label.
    public var id: String { label }

    /// The value as one line of text: the code and the text joined by " · ".
    public var value: String {
        [code, text].compactMap { $0 }.joined(separator: " · ")
    }

    init(_ label: String, code: String? = nil, text: String? = nil) {
        self.label = label
        self.code = code
        self.text = text
    }
}

/// Everything the Connection section of Diagnostics shows. It is a pure value, so the
/// rows and Copy Details can be tested without a daemon.
///
/// It never holds the value of `STACKLY_ROOT`.
nonisolated public struct ConnectionDiagnostics: Hashable, Sendable {
    /// The daemon's state.
    public var status: ShellStatus
    /// The socket path the app connects to, or `nil` when it could not be worked out.
    public var socketPath: String?
    /// The last connection error, or `nil`.
    public var lastError: DiagnosticsError?
    /// When the next connection attempt runs, while one waits.
    public var nextAttempt: Date?
    /// When the app last connected, or `nil` when it never did.
    public var lastConnectedAt: Date?
    /// What the last known daemon reported about itself.
    public var daemon: DaemonFacts?
    /// The daemon's protocol in `M.m` form, from the last handshake or mismatch.
    public var daemonProtocol: String?
    /// This app's protocol in `M.m` form.
    public var appProtocol: String?
    /// Whether the last handshake reported contract drift, or `nil` before any.
    public var contractDrift: Bool?
    /// The daemon's health from the last heartbeat.
    public var health: HealthFacts?
    /// What the agent's banner row is built from, only while the daemon is unreachable
    /// with `ipc.endpoint_missing` or `ipc.connection_refused`. The What to do row takes
    /// its words from the row, so it never differs from the banner (spec 0015).
    public var agentHint: AgentBanner.Input?

    /// Creates the diagnostics.
    public init(
        status: ShellStatus,
        socketPath: String? = nil,
        lastError: DiagnosticsError? = nil,
        nextAttempt: Date? = nil,
        lastConnectedAt: Date? = nil,
        daemon: DaemonFacts? = nil,
        daemonProtocol: String? = nil,
        appProtocol: String?,
        contractDrift: Bool? = nil,
        health: HealthFacts? = nil,
        agentHint: AgentBanner.Input? = nil
    ) {
        self.status = status
        self.socketPath = socketPath
        self.lastError = lastError
        self.nextAttempt = nextAttempt
        self.lastConnectedAt = lastConnectedAt
        self.daemon = daemon
        self.daemonProtocol = daemonProtocol
        self.appProtocol = appProtocol
        self.contractDrift = contractDrift
        self.health = health
        self.agentHint = agentHint
    }

    /// The rows of the Connection section, in order. A row without a value is left out,
    /// except Last error ("None") and Last connected ("Never").
    ///
    /// Views pass the system values; tests pass fixed ones.
    ///
    /// - Parameters:
    ///   - calendar: The calendar of the times.
    ///   - locale: The locale of the times.
    ///   - timeZone: The time zone of the times.
    ///   - now: The time the agent hint is worked out at, for its 15 second limit.
    public func rows(
        calendar: Calendar, locale: Locale, timeZone: TimeZone, now: Date = Date()
    ) -> [DiagnosticsRow] {
        func time(_ date: Date) -> String {
            DiagnosticsTime.format(date, calendar: calendar, locale: locale, timeZone: timeZone)
        }
        let presentation = ShellPresentation(status: status)
        var rows = [
            DiagnosticsRow("Status", text: presentation.footer.label),
            socketPath.map { DiagnosticsRow("Socket", code: $0) }
                ?? DiagnosticsRow("Socket", text: "Unavailable"),
            lastErrorRow,
        ]
        let agentText = agentHint.flatMap { AgentBanner.hint($0, now: now) }
        if let hint = ConnectionHint.text(status: status, error: lastError, agent: agentText) {
            rows.append(DiagnosticsRow("What to do", text: hint))
        }
        if let nextAttempt, presentation.hasConnection == false {
            rows.append(DiagnosticsRow("Next attempt", text: time(nextAttempt)))
        }
        rows.append(DiagnosticsRow("Last connected", text: lastConnectedAt.map(time) ?? "Never"))
        if let daemon {
            let facts = [
                daemon.version,
                "pid \(daemon.pid)",
                "started \(time(daemon.startedAt))",
                "instance \(daemon.serverInstanceId)",
            ].joined(separator: ", ")
            let suffix = presentation.hasConnection ? "" : " (last known)"
            rows.append(DiagnosticsRow("stacklyd", text: facts + suffix))
        }
        let protocols = [
            daemonProtocol.map { "stacklyd \($0)" },
            appProtocol.map { "Stackly \($0)" },
        ].compactMap { $0 }
        if protocols.isEmpty == false {
            rows.append(DiagnosticsRow("Protocol", text: protocols.joined(separator: " · ")))
        }
        if let contractDrift {
            let text = contractDrift ? "Different build (contract drift)" : "Matches"
            rows.append(DiagnosticsRow("Contract", text: text))
        }
        if let health {
            rows.append(DiagnosticsRow("Health", text: Self.healthText(health)))
        }
        return rows
    }

    /// The text Copy Details puts on the pasteboard: one `Label: value` line per row, the
    /// same rows the section shows.
    ///
    /// - Parameter rows: The rows the section shows.
    public static func copyText(_ rows: [DiagnosticsRow]) -> String {
        rows.map { "\($0.label): \($0.value)" }.joined(separator: "\n")
    }

    private var lastErrorRow: DiagnosticsRow {
        guard let lastError else {
            return DiagnosticsRow("Last error", text: "None")
        }
        let parts = [lastError.category, lastError.message] + [lastError.reason].compactMap { $0 }
        return DiagnosticsRow(
            "Last error",
            code: lastError.code,
            text: parts.joined(separator: " · ")
        )
    }

    private static func healthText(_ health: HealthFacts) -> String {
        let subsystems = health.subsystems.map { subsystem in
            let detail = subsystem.detail.map { ": \($0)" } ?? ""
            return "\(subsystem.name) \(subsystem.status)\(detail)"
        }
        return ([health.status] + subsystems).joined(separator: " · ")
    }
}
