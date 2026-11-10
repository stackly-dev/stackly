import Foundation
import StacklyClient

/// The free port a failed start suggested for one endpoint (spec 0033, AC-4 and AC-10).
nonisolated public struct PortFix: Hashable, Sendable {
    /// The endpoint name, such as `database`.
    public let endpoint: String
    /// The port the daemon found free.
    public let port: UInt16

    /// Creates a fix.
    public init(endpoint: String, port: UInt16) {
        self.endpoint = endpoint
        self.port = port
    }
}

/// What the failure banner of an instance shows (spec 0033, *State banner* and *Copy banner*):
/// a pure function of the summary and the related operations, with no view in it. The Summary
/// tab and the inspector both draw from it.
nonisolated public struct FailureBannerContent: Equatable, Sendable {
    /// The rule of *State banner* that matched first.
    public enum Rule: String, Equatable, Sendable, CaseIterable {
        /// Lifecycle `recovering`: a restart waits (AC-12).
        case restarting = "F1"
        /// Lifecycle `failed` before the data folder was initialized.
        case initFailed = "F2"
        /// Lifecycle `failed` because the restart budget ran out (AC-9).
        case crashLoop = "F3"
        /// A port could not be used (AC-10).
        case port = "F4"
        /// Another process holds the data lock.
        case dataInUse = "F5"
        /// Lifecycle `failed` for another reason (AC-11).
        case failed = "F6"
        /// Lifecycle `stopped` after a start or restart that the daemon refused (AC-11).
        case startRefused = "F7"
    }

    /// A button of the banner. The first one of ``actions`` is the main button.
    public enum Action: Hashable, Sendable {
        /// "Use Port {n}": opens the Ports sheet with the port filled in.
        case usePort(PortFix)
        /// "Change Port…": opens the Ports sheet with nothing filled in.
        case changePort
        /// "Show Logs": the Logs tab of the instance.
        case showLogs
        /// "Retry": the main Retry of spec 0022, one new `instance.start`.
        case retry
        /// "Diagnose…": Diagnostics with the focus on the instance.
        case diagnose

        /// The button title.
        public var title: String {
            switch self {
            case .usePort(let fix): "Use Port \(fix.port)"
            case .changePort: "Change Port…"
            case .showLogs: "Show Logs"
            case .retry: "Retry"
            case .diagnose: "Diagnose…"
            }
        }
    }

    /// The rule that matched.
    public let rule: Rule
    /// Why it failed, or `nil` for a restart.
    public let cause: FailureCause?
    /// The tone of the banner: `caution` for a restart, `critical` otherwise.
    public let tone: Tone
    /// The title.
    public let title: String
    /// The sentence under the title.
    public let message: String
    /// The hint of the daemon, as the second sentence, or `nil`.
    public let hint: String?
    /// Whether a port change succeeded after the failure, so the banner says "Its ports
    /// changed. Retry to start with them." and offers no port button.
    public let portsChanged: Bool
    /// The code of the code block: the error code of the operation first, then
    /// `lastFailure.code`; `nil` for a restart or when there is none.
    public let code: String?
    /// The output of the failed process, one line each.
    public let excerpt: [String]
    /// The operation that failed, the correlation ID of the error (AC-5).
    public let operationId: UUID?
    /// The suggested port, when the error carries the endpoint and a free port.
    public let portFix: PortFix?
    /// The buttons of the Summary tab, the main one first.
    public let actions: [Action]

    /// The sentence a successful port change adds (AC-10).
    public static let portsChangedNote = "Its ports changed. Retry to start with them."

    /// The first line of the code block: "{code} · operation {uuid}", only the code without an
    /// operation, or `nil` when there is no code.
    public var codeLine: String? {
        FailureBannerCopy.codeLine(code: code, operation: operationId)
    }

    /// The text of the code block of the Summary tab: the code line, then the excerpt.
    public var summaryCode: String? {
        let lines = [codeLine].compactMap { $0 } + excerpt
        return lines.isEmpty ? nil : lines.joined(separator: "\n")
    }

    /// The text of the code block of the inspector: the excerpt, or the code without the
    /// operation when there is no excerpt.
    public var inspectorCode: String? {
        if excerpt.isEmpty == false { return excerpt.joined(separator: "\n") }
        return code
    }

    /// The button of the inspector: only Use Port, for a port problem with a suggestion.
    public var inspectorActions: [Action] {
        actions.filter {
            if case .usePort = $0 { return true }
            return false
        }
    }
}

extension FailureBannerContent {
    /// The banner of `summary`, or `nil` when it gets none: a running or stopped instance
    /// without a refused start, an instance whose banner is I2 to I4 of spec 0022 (unknown,
    /// a process not confirmed, data in use, a missing secret), and a retained one.
    /// - Parameters:
    ///   - summary: The instance.
    ///   - related: Its operations, newest or not; an empty list gives the banner of the summary
    ///     alone, without an operation ID or Use Port.
    ///   - instanceNames: The name of another instance by ID, for a port it owns.
    ///   - now: The clock of the restart countdown.
    public static func resolve(
        _ summary: InstanceSummary, related: [OperationSummary],
        instanceNames: (UUID) -> String?, now: Date
    ) -> FailureBannerContent? {
        if summary.lifecycle != .failed, isBlocked(summary) { return nil }
        let reading = RelatedOperation.reading(of: summary, operations: related)
        let error = reading.error
        let cause =
            error.map { FailureCause(errorCode: $0.code) }
            ?? summary.lastFailure.map { FailureCause(failureCode: $0.code) }
        let rule: Rule
        switch summary.lifecycle {
        case .recovering:
            rule = .restarting
        case .failed:
            if summary.initialized == false {
                rule = .initFailed
            } else {
                switch cause {
                case .crashLoop: rule = .crashLoop
                case .port: rule = .port
                case .dataInUse: rule = .dataInUse
                case .other, nil: rule = .failed
                }
            }
        case .stopped:
            guard error != nil else { return nil }
            switch cause {
            case .port: rule = .port
            case .dataInUse: rule = .dataInUse
            case .crashLoop, .other, nil: rule = .startRefused
            }
        default:
            return nil
        }
        return build(
            rule, cause: cause, summary: summary, reading: reading,
            instanceNames: instanceNames, now: now)
    }

    /// Whether the instance keeps banner I2, I3, or I4 of spec 0022.
    private static func isBlocked(_ summary: InstanceSummary) -> Bool {
        if summary.lifecycle == .unknown { return true }
        switch summary.blocker {
        case .identityUnproven, .dataInUse, .needsSecret: return true
        case .configDrift, nil: return false
        }
    }

    // MARK: - Content

    private static func build(
        _ rule: Rule, cause: FailureCause?, summary: InstanceSummary,
        reading: RelatedOperation.Reading, instanceNames: (UUID) -> String?, now: Date
    ) -> FailureBannerContent {
        let error = reading.error
        let failure = summary.lastFailure
        let failureText = InstanceFailureCopy.text(failure, restartBudget: summary.restart.budget)
        let title: String
        let message: String
        switch rule {
        case .restarting:
            title = "Restarting after a crash"
            message = FailureBannerCopy.restartText(summary.restart, now: now)
        case .initFailed:
            title = "Initialization failed"
            message = failureText
        case .crashLoop:
            title = "Crash loop. Automatic restarts paused."
            message = FailureBannerCopy.crashLoopText(summary.restart)
        case .port:
            let problem: FailureCause.PortProblem
            if case .port(let found) = cause { problem = found } else { problem = .inUse }
            title =
                problem == .unavailable
                ? "Couldn’t start: port unavailable" : "Couldn’t start: port in use"
            message =
                FailureBannerCopy.portText(problem, details: error, instanceNames: instanceNames)
                ?? error.map { DaemonText.sentence($0.message) } ?? failureText
        case .dataInUse:
            title = "Couldn’t start: data in use"
            message = "Another process holds its data lock."
        case .failed:
            title = "\(summary.name) failed"
            message = failureText
        case .startRefused:
            title = "Couldn’t start \(summary.name)"
            message = error.map { DaemonText.sentence($0.message) } ?? failureText
        }
        let operationId = reading.operation.map { $0.failure?.error.operationId ?? $0.id }
        let portFix = reading.portsChanged ? nil : FailureBannerCopy.portFix(of: error)
        return FailureBannerContent(
            rule: rule, cause: rule == .restarting ? nil : cause,
            tone: rule == .restarting ? .caution : .critical, title: title, message: message,
            hint: error?.hint.flatMap(FailureBannerCopy.hint),
            portsChanged: reading.portsChanged,
            code: rule == .restarting ? nil : error?.code ?? failure?.code,
            excerpt: summary.lifecycle == .stopped ? [] : FailureBannerCopy.lines(of: failure),
            operationId: operationId, portFix: portFix,
            actions: actions(rule, portFix: portFix, portsChanged: reading.portsChanged))
    }

    /// The buttons of *Copy banner*, the main one first.
    private static func actions(_ rule: Rule, portFix: PortFix?, portsChanged: Bool) -> [Action] {
        switch rule {
        case .restarting:
            return [.showLogs]
        case .initFailed:
            return [.showLogs, .diagnose]
        case .port where portsChanged == false:
            return [portFix.map(Action.usePort) ?? .changePort, .showLogs, .retry, .diagnose]
        case .crashLoop, .port, .dataInUse, .failed, .startRefused:
            return [.retry, .showLogs, .diagnose]
        }
    }
}

/// The words of the failure banner that come from numbers and details (spec 0033, *Copy
/// banner*). Every other sentence is fixed text, a catalog detail, or the excerpt the daemon
/// already redacted; nothing here is built from process output.
nonisolated enum FailureBannerCopy {
    /// "Restart {attempts} of {budget} in {s} s." while a restart waits inside the budget,
    /// else "Stackly is starting it again.". `attempts` already counts the crash that started
    /// the pause, so it is the number of the respawn that waits.
    static func restartText(_ restart: InstanceRestart, now: Date) -> String {
        guard let next = restart.nextAttemptAt, restart.attempts <= restart.budget else {
            return "Stackly is starting it again."
        }
        return
            "Restart \(restart.attempts) of \(restart.budget) in \(seconds(until: next, now: now)) s."
    }

    /// The whole seconds until `date`, rounded up, never below zero.
    static func seconds(until date: Date, now: Date) -> Int {
        max(0, Int(date.timeIntervalSince(now).rounded(.up)))
    }

    /// "It crashed {attempts} times in {window}, so…" while the window holds crashes, else
    /// the sentence without numbers (after a daemon restart).
    static func crashLoopText(_ restart: InstanceRestart) -> String {
        guard restart.attempts > 0 else {
            return "It kept crashing, so Stackly stopped restarting it and is waiting for you."
        }
        let times = restart.attempts == 1 ? "1 time" : "\(restart.attempts) times"
        return
            "It crashed \(times) in \(window(restart.windowSeconds)), so Stackly stopped restarting it and is waiting for you."
    }

    /// "5 minutes" for 300 seconds, minutes rounded up, "1 minute" for one.
    static func window(_ seconds: UInt32) -> String {
        let minutes = (seconds + 59) / 60
        return minutes == 1 ? "1 minute" : "\(minutes) minutes"
    }

    /// The sentence of a port problem from the details of the error, or `nil` when they do not
    /// name the port and the endpoint.
    static func portText(
        _ problem: FailureCause.PortProblem, details error: ErrorBody?,
        instanceNames: (UUID) -> String?
    ) -> String? {
        guard let error, let port = detail("port", of: error),
            let endpoint = detail("endpoint", of: error)
        else { return nil }
        switch problem {
        case .inUse:
            return "Port \(port) of \(endpoint) is held by another process."
        case .conflict:
            let other =
                detail("otherInstance", of: error).flatMap(UUID.init(uuidString:)).flatMap(
                    instanceNames) ?? "another instance"
            return "Port \(port) of \(endpoint) belongs to \(other)."
        case .unavailable:
            return "macOS refused port \(port) of \(endpoint)."
        }
    }

    /// The fix of an error that names the endpoint and a free port.
    static func portFix(of error: ErrorBody?) -> PortFix? {
        guard let error, case .port = FailureCause(errorCode: error.code),
            let endpoint = detail("endpoint", of: error),
            let port = detail("suggestedPort", of: error).flatMap(UInt16.init), port > 0
        else { return nil }
        return PortFix(endpoint: endpoint, port: port)
    }

    /// The hint as a sentence, or `nil` when it is empty.
    static func hint(_ text: String) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : DaemonText.sentence(trimmed)
    }

    /// "{code} · operation {uuid}", only the code, or `nil` without a code.
    static func codeLine(code: String?, operation: UUID?) -> String? {
        guard let code else { return nil }
        guard let operation else { return code }
        return "\(code) · operation \(operation.uuidString.lowercased())"
    }

    /// The lines of the excerpt, one per line of output.
    static func lines(of failure: InstanceLastFailure?) -> [String] {
        (failure?.excerpt ?? []).flatMap {
            $0.split(whereSeparator: \.isNewline).map(String.init)
        }
    }

    private static func detail(_ key: String, of error: ErrorBody) -> String? {
        error.details?.first { $0.key == key }?.value
    }
}
