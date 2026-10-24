import Foundation

/// The window banner for the background agent while the daemon is unreachable (spec 0015, *Banner per state*).
nonisolated public enum AgentBanner {
    /// What the banner is built from.
    nonisolated public struct Input: Hashable, Sendable {
        /// The shell's status. A `refreshing` status is opened to the one before it.
        public var status: ShellStatus
        /// The code of the last failed attempt while the status stayed unreachable.
        public var failureCode: String?
        /// The agent's state, or `nil` before the first one.
        public var assessment: AgentAssessment?
        /// The detail of an unavailable status, such as "timed out".
        public var detail: String?
        /// Whether Turn On or Start stacklyd is waiting for the daemon.
        public var isStarting: Bool

        /// Creates the input.
        public init(
            status: ShellStatus,
            failureCode: String?,
            assessment: AgentAssessment?,
            detail: String? = nil,
            isStarting: Bool = false
        ) {
            self.status = status
            self.failureCode = failureCode
            self.assessment = assessment
            self.detail = detail
            self.isStarting = isStarting
        }
    }

    /// The two connection errors the agent banner answers.
    static let failureCodes: Set<String> = ["ipc.endpoint_missing", "ipc.connection_refused"]

    /// The banner for `input` at `now`, or `nil` when the daemon is not unreachable with one
    /// of the two codes, or no state is known yet (the banner of spec 0013 applies then).
    public static func content(_ input: Input, now: Date) -> DaemonBannerContent? {
        var status = input.status
        while case .refreshing(let previous) = status { status = previous }
        guard case .unreachable(let lastKnownAt, let since) = status,
            let code = input.failureCode, failureCodes.contains(code),
            let assessment = input.assessment
        else {
            return nil
        }
        let row = row(for: assessment, input: input, now: now, since: since)
        let message: DaemonBannerContent.Message =
            lastKnownAt.map { .lastKnownState(since: $0, then: row.message) }
            ?? .text(row.message)
        return DaemonBannerContent(
            tone: row.tone, title: row.title, message: message, actions: row.actions)
    }

    /// The title and the message of the banner row for `input` as one hint, without the
    /// "last known state" note, or `nil` when ``content(_:now:)`` has no row.
    public static func hint(_ input: Input, now: Date) -> String? {
        var status = input.status
        while case .refreshing(let previous) = status { status = previous }
        guard case .unreachable(_, let since) = status,
            let code = input.failureCode, failureCodes.contains(code),
            let assessment = input.assessment
        else {
            return nil
        }
        let row = row(for: assessment, input: input, now: now, since: since)
        return "\(row.title) \(row.message)"
    }

    /// Whether the banner for `input` changes at the second `startGrace` passes, so its
    /// view redraws every second until then (spec 0013).
    public static func isCountingToGrace(_ input: Input?, now: Date) -> Bool {
        guard let input, input.isStarting == false, let assessment = input.assessment,
            assessment.state == .enabled
        else {
            return false
        }
        var status = input.status
        while case .refreshing(let previous) = status { status = previous }
        guard case .unreachable(_, let since) = status,
            let code = input.failureCode, failureCodes.contains(code)
        else {
            return false
        }
        return now.timeIntervalSince(since) < graceSeconds
    }

    private static let graceSeconds: TimeInterval = 15

    private struct Row {
        var tone: Tone
        var title: String
        var message: String
        var actions: [DaemonBannerAction]
    }

    private static func row(
        for assessment: AgentAssessment, input: Input, now: Date, since: Date
    ) -> Row {
        switch assessment.state {
        case .notRegistered:
            Row(
                tone: .caution, title: "Stackly’s background service is off.",
                message:
                    "Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it.",
                actions: [.turnOn, .openDiagnostics])
        case .requiresApproval:
            Row(
                tone: .caution, title: "Allow Stackly to run in the background.",
                message:
                    "Turn on Stackly in System Settings > General > Login Items & Extensions. Stackly doesn’t change this setting for you.",
                actions: [.openLoginItems, .checkAgain])
        case .disabled:
            Row(
                tone: .caution, title: "Stackly’s background service is turned off.",
                message:
                    "Turn on Stackly in System Settings > General > Login Items & Extensions to run stacklyd again.",
                actions: [.openLoginItems, .checkAgain])
        case .enabled:
            if input.isStarting || now.timeIntervalSince(since) < graceSeconds {
                Row(
                    tone: .informative, title: "stacklyd is starting…",
                    message: "Actions are available once it’s ready.", actions: [])
            } else {
                Row(
                    tone: .caution, title: "stacklyd isn’t running.",
                    message:
                        "It was stopped or it exited. Start it again here, or run stackly daemon start in Terminal.",
                    actions: [.startDaemon, .openDiagnostics])
            }
        case .notFound:
            Row(
                tone: .critical, title: "Stackly can’t find its background service.",
                message:
                    "macOS couldn’t find stacklyd for this copy of Stackly. Move Stackly to the Applications folder, or install it again.",
                actions: [.openDiagnostics])
        case .unavailable:
            unavailableRow(assessment.reason, detail: input.detail)
        }
    }

    private static func unavailableRow(_ reason: UnavailableReason?, detail: String?) -> Row {
        switch reason {
        case .helperMissing, .notBundled:
            Row(
                tone: .caution, title: "This build doesn’t include stacklyd.",
                message: "Start stacklyd yourself, or build the app with cargo xtask build-app.",
                actions: [.reconnect, .openDiagnostics])
        case .rootOverride:
            Row(
                tone: .caution, title: "STACKLY_ROOT is set.",
                message:
                    "The background service only runs the default Stackly folder. Start stacklyd yourself for this folder.",
                actions: [.reconnect, .openDiagnostics])
        case .statusFailed, .statusUnrecognized, .unsupportedOs, nil:
            Row(
                tone: .caution, title: "Stackly couldn’t check its background service.",
                message: detail.map { "macOS didn’t answer (\($0)). Check again in a moment." }
                    ?? "macOS didn’t answer. Check again in a moment.",
                actions: [.checkAgain, .openDiagnostics])
        }
    }
}
