import Foundation
import StacklyClient

/// What a click on an instance's main button asks the daemon for.
nonisolated public enum InstanceVerb: String, Hashable, Sendable {
    /// `instance.start`, for the Start and Retry buttons.
    case start
    /// `instance.stop`.
    case stop

    /// "start" or "stop", for the copy of a feedback card.
    var word: String { rawValue }

    var progressive: String {
        switch self {
        case .start: "Starting"
        case .stop: "Stopping"
        }
    }
}

/// What this window knows about the click it sent for an instance.
nonisolated public enum InstanceLocalStatus: Hashable, Sendable {
    /// Nothing is pending.
    case none
    /// The click is sent and the reply has not come.
    case inFlight(InstanceVerb)
    /// The daemon accepted the click as `operationId` and it has not ended.
    case pending(InstanceVerb, operationId: UUID)

    /// The verb of a click that is in flight or pending, or `nil`.
    public var verb: InstanceVerb? {
        switch self {
        case .none: nil
        case .inFlight(let verb), .pending(let verb, _): verb
        }
    }
}

/// The main action of an instance: which button it is, whether it can run, and why not
/// (spec 0022, *Aksi instance*). It is a pure function of the summary and the local status.
nonisolated public struct InstanceAction: Equatable, Sendable {
    /// Which button it is. Its raw value is the ID of the ``StacklyAction``.
    public enum Kind: String, Sendable {
        /// Start a stopped instance.
        case start
        /// Stop a running instance.
        case stop
        /// Start a failed instance again.
        case retry
        /// A change is going on; no button works.
        case busy
    }

    /// The button.
    public let kind: Kind
    /// The title of the button.
    public let title: String
    /// The SF Symbol of the button, or `nil` for a busy one.
    public let symbol: String?
    /// The verb a click sends; `nil` when the button cannot run.
    public let verb: InstanceVerb?
    /// Why the button cannot run, or `nil`.
    public let disabledReason: String?
    /// Whether a change is going on.
    public let isBusy: Bool
    /// The Instance menu command this action stands for: Start and Retry are `start`, Stop
    /// is `stop`, and a busy one is the verb that is going on. `nil` for a busy state only
    /// the daemon changes (none today).
    public let commandVerb: InstanceVerb?

    /// The reason a failed instance whose data folder was never initialized gives.
    static let notInitializedRetry =
        "Initialization failed. Remove this instance and create it again."

    /// What the main action of `summary` is.
    /// - Parameters:
    ///   - summary: The instance.
    ///   - local: What this window knows about its own click.
    public static func resolve(summary: InstanceSummary, local: InstanceLocalStatus)
        -> InstanceAction
    {
        if let verb = local.verb {
            return busy("\(verb.progressive)…", verb: verb)
        }
        switch summary.lifecycle {
        case .stopped:
            return InstanceAction(
                kind: .start, title: "Start", symbol: "play.fill", verb: .start,
                disabledReason: nil, isBusy: false, commandVerb: .start)
        case .failed:
            let initialized = summary.initialized != false
            return InstanceAction(
                kind: .retry, title: "Retry", symbol: "arrow.clockwise",
                verb: initialized ? .start : nil,
                disabledReason: initialized ? nil : notInitializedRetry, isBusy: false,
                commandVerb: .start)
        case .running, .recovering:
            return InstanceAction(
                kind: .stop, title: "Stop", symbol: "stop.fill", verb: .stop,
                disabledReason: nil, isBusy: false, commandVerb: .stop)
        case .initializing:
            return busy("Initializing…", verb: .start)
        case .starting:
            return busy("Starting…", verb: .start)
        case .stopping:
            return busy("Stopping…", verb: .stop)
        case .unknown:
            return InstanceAction(
                kind: .stop, title: "Stop", symbol: "stop.fill", verb: nil,
                disabledReason: "Stackly can’t confirm this process.", isBusy: false,
                commandVerb: .stop)
        case .uninitialized:
            return InstanceAction(
                kind: .start, title: "Start", symbol: "play.fill", verb: nil,
                disabledReason: "This instance isn’t initialized.", isBusy: false,
                commandVerb: .start)
        case .retained:
            return InstanceAction(
                kind: .start, title: "Start", symbol: "play.fill", verb: nil,
                disabledReason: "This instance was removed.", isBusy: false,
                commandVerb: .start)
        }
    }

    private static func busy(_ title: String, verb: InstanceVerb) -> InstanceAction {
        InstanceAction(
            kind: .busy, title: title, symbol: nil, verb: nil, disabledReason: nil, isBusy: true,
            commandVerb: verb)
    }
}
