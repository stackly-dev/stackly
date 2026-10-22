import Foundation

/// The call that was running when macOS failed or timed out.
nonisolated public enum AgentOp: String, Sendable, Equatable, Hashable {
    /// Reading the status.
    case status
    /// Registering the agent.
    case register
    /// Unregistering the agent.
    case unregister
    /// Registering again to start the job.
    case relaunch

    /// The verb phrase of the error text: "while trying to {phrase}".
    var phrase: String {
        switch self {
        case .status: "read the status"
        case .register: "register"
        case .unregister: "unregister"
        case .relaunch: "start the service"
        }
    }
}

/// How the OS error of a registration call maps (spec 0015, *Hasil pendaftaran*).
nonisolated public enum AgentOsErrorKind: String, Sendable, Equatable, Hashable {
    /// `kSMErrorAlreadyRegistered`.
    case alreadyRegistered
    /// `kSMErrorLaunchDeniedByUser`.
    case launchDenied
    /// `kSMErrorJobMustBeEnabled`.
    case mustBeEnabled
    /// `kSMErrorInvalidSignature`.
    case invalidSignature
    /// `kSMErrorJobNotFound` or `kSMErrorJobPlistNotFound`.
    case notFound
    /// Any other code.
    case other
}

/// What went wrong in an action on the background agent (spec 0015, *GUI*). It never
/// becomes an `ErrorBody`; Diagnostics shows its text as Last error.
nonisolated public enum AgentActionError: Error, Sendable, Equatable, Hashable {
    /// macOS returned an error in `domain` with `code`.
    case os(op: AgentOp, kind: AgentOsErrorKind, domain: String, code: Int)
    /// macOS did not answer in time.
    case timedOut(op: AgentOp)
    /// `stacklyd` did not answer within `seconds` after the service started.
    case daemonDidNotStart(seconds: Int)
    /// macOS accepted the registration and then did not keep it.
    case registrationNotKept

    /// The text Diagnostics shows as Last error.
    public var message: String {
        switch self {
        case .os(_, .invalidSignature, _, _):
            "macOS rejected the signature of this build."
        case .os(let op, _, let domain, let code):
            "macOS returned \(domain) \(code) while trying to \(op.phrase)."
        case .timedOut:
            "macOS didn’t answer in time."
        case .daemonDidNotStart(let seconds):
            "stacklyd didn’t start within \(seconds) seconds."
        case .registrationNotKept:
            "macOS didn’t keep the registration."
        }
    }
}
