import Foundation

/// What the Background Agent section of Diagnostics shows, as a pure value so the rows and
/// Copy Details can be tested without a model (spec 0015, *GUI*).
nonisolated public struct AgentDiagnostics: Hashable, Sendable {
    /// The agent's state, or `nil` before the first status.
    public var assessment: AgentAssessment?
    /// The canonical path of the bundle the OS answered for.
    public var bundlePath: String?
    /// When launchd last started the daemon.
    public var lastLaunchedAt: Date?
    /// When the status was last read.
    public var lastCheckedAt: Date?
    /// What the last action failed with.
    public var lastError: AgentActionError?
    /// The detail of an unavailable status, such as "timed out".
    public var detail: String?

    /// Creates the value.
    public init(
        assessment: AgentAssessment?, bundlePath: String? = nil, lastLaunchedAt: Date? = nil,
        lastCheckedAt: Date? = nil, lastError: AgentActionError? = nil, detail: String? = nil
    ) {
        self.assessment = assessment
        self.bundlePath = bundlePath
        self.lastLaunchedAt = lastLaunchedAt
        self.lastCheckedAt = lastCheckedAt
        self.lastError = lastError
        self.detail = detail
    }

    /// The words for the state in the State row.
    public static func text(for assessment: AgentAssessment) -> String {
        switch assessment.state {
        case .enabled: "On"
        case .requiresApproval: "Needs approval in Login Items"
        case .disabled: "Turned off in Login Items"
        case .notRegistered: "Off"
        case .notFound: "Not found"
        case .unavailable:
            switch assessment.reason {
            case .rootOverride: "Unavailable · STACKLY_ROOT is set"
            case .unsupportedOs: "Unavailable · this macOS can’t run it"
            case .statusFailed: "Unavailable · macOS didn’t answer"
            case .notBundled: "Unavailable · not running from Stackly.app"
            case .helperMissing: "Unavailable · this build has no stacklyd"
            case .statusUnrecognized: "Unavailable · unknown status"
            case nil: "Unavailable"
            }
        }
    }

    /// The rows of the section, in order: State, What to do (when there is something to
    /// do), App, Label, Last launched by launchd, Last checked, and Last error.
    ///
    /// Views pass the system values; tests pass fixed ones.
    public func rows(calendar: Calendar, locale: Locale, timeZone: TimeZone) -> [DiagnosticsRow] {
        func time(_ date: Date?) -> String {
            date.map {
                DiagnosticsTime.format($0, calendar: calendar, locale: locale, timeZone: timeZone)
            } ?? "Never"
        }
        var state = assessment.map(Self.text(for:)) ?? "Unknown"
        if let detail, assessment?.state == .unavailable { state += " (\(detail))" }
        var rows = [DiagnosticsRow("State", text: state)]
        if let step = assessment?.recovery.text {
            rows.append(DiagnosticsRow("What to do", text: step))
        }
        rows += [
            bundlePath.map { DiagnosticsRow("App", code: $0) }
                ?? DiagnosticsRow("App", text: "Unknown"),
            DiagnosticsRow("Label", code: AgentIdentifiers.daemonLabel),
            DiagnosticsRow("Last launched by launchd", text: time(lastLaunchedAt)),
            DiagnosticsRow("Last checked", text: time(lastCheckedAt)),
            DiagnosticsRow("Last error", text: lastError?.message ?? "None"),
        ]
        return rows
    }
}
