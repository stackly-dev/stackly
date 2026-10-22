import Foundation

/// What the OS says about the registration of the background agent, before it is turned
/// into a ``BackgroundAgentState`` (spec 0015, *Port `BackgroundAgent`*).
nonisolated public enum AgentRegistration: Sendable, Equatable {
    /// Registered and allowed to run.
    case enabled
    /// Registered, and the user still has to allow it in System Settings.
    case requiresApproval
    /// Unregistered by an earlier `unregister`.
    case notRegistered
    /// A bundle that was never registered, or a service the OS cannot find.
    case notFound
    /// A status value this app does not know.
    case unrecognized(Int)
}

/// The result of asking the OS about the agent, with the bundle it asked about.
nonisolated public enum AgentProbe: Sendable, Equatable {
    /// The OS answered for the bundle at `bundle` (a canonical path).
    case registration(AgentRegistration, bundle: String)
    /// The app does not run from a bundle with the app's identifier.
    case notBundled(executable: String)
    /// The bundle at `bundle` has no LaunchAgent plist or no `stacklyd`.
    case helperMissing(bundle: String)
}

/// The probe as `classify` reads it, so a missing OS call and a failed one stay apart.
nonisolated public enum ProbeOutcome: Sendable, Equatable {
    /// The OS was not asked, because `STACKLY_ROOT` is set.
    case notChecked
    /// This macOS cannot register agents.
    case unsupported
    /// The status call failed with `code`, or did not answer in time (`nil`).
    case failed(code: Int?)
    /// The OS answered.
    case probe(AgentProbe)
}

/// The six states of the background agent (spec 0015, *State background agent*). The raw
/// values are the snake case names of the wire and of `protocols/fixtures/agent/states.json`.
nonisolated public enum BackgroundAgentState: String, Sendable, Equatable, CaseIterable {
    /// Registered and allowed.
    case enabled
    /// Registered; the user has not allowed it yet.
    case requiresApproval = "requires_approval"
    /// The user turned it off in System Settings.
    case disabled
    /// Not registered.
    case notRegistered = "not_registered"
    /// The OS accepted the registration and still cannot find the service.
    case notFound = "not_found"
    /// The state cannot be read, or the agent does not apply to this build.
    case unavailable
}

/// Why the state is ``BackgroundAgentState/unavailable``.
nonisolated public enum UnavailableReason: String, Sendable, Equatable, CaseIterable {
    /// `STACKLY_ROOT` is set, so the agent does not apply.
    case rootOverride = "root_override"
    /// This macOS cannot register agents.
    case unsupportedOs = "unsupported_os"
    /// The status call failed or timed out.
    case statusFailed = "status_failed"
    /// The app does not run from a Stackly bundle.
    case notBundled = "not_bundled"
    /// The bundle has no plist or no `stacklyd`.
    case helperMissing = "helper_missing"
    /// The OS returned a status this app does not know.
    case statusUnrecognized = "status_unrecognized"
}

/// What the user can do next about the agent.
nonisolated public enum RecoveryStep: String, Sendable, Equatable, CaseIterable {
    /// Nothing.
    case none
    /// Start the daemon (the caller adds this when the daemon is unreachable).
    case start
    /// Register the agent.
    case register
    /// Allow Stackly in Login Items.
    case approveInLoginItems = "approve_in_login_items"
    /// Turn Stackly back on in Login Items.
    case enableInLoginItems = "enable_in_login_items"
    /// Move the app to the Applications folder or install it again.
    case moveOrReinstallApp = "move_or_reinstall_app"
    /// Use the command line tool inside the bundle.
    case useBundledCli = "use_bundled_cli"
    /// Start `stacklyd` by hand for the `STACKLY_ROOT` folder.
    case manualDaemonForOverride = "manual_daemon_for_override"
    /// Build the app with `cargo xtask build-app`.
    case buildWithXtask = "build_with_xtask"
    /// Check again in a moment.
    case checkAgain = "check_again"

    /// The words Diagnostics shows for the step (What to do), or `nil` when there is
    /// nothing to do.
    public var text: String? {
        switch self {
        case .none:
            nil
        case .start:
            "Start stacklyd."
        case .register:
            "Turn on the background service."
        case .approveInLoginItems:
            "Turn on Stackly in System Settings > General > Login Items & Extensions."
        case .enableInLoginItems:
            "Turn Stackly back on in System Settings > General > Login Items & Extensions."
        case .moveOrReinstallApp:
            "Move Stackly to the Applications folder, or install it again."
        case .useBundledCli:
            "Start stacklyd yourself, or run Stackly from its app bundle."
        case .manualDaemonForOverride:
            "Start stacklyd yourself for the STACKLY_ROOT folder."
        case .buildWithXtask:
            "Build the app with cargo xtask build-app."
        case .checkAgain:
            "Check again in a moment."
        }
    }
}

/// What `classify` reads.
nonisolated public struct AgentInput: Sendable, Equatable {
    /// Whether `STACKLY_ROOT` is set.
    public var rootOverride: Bool
    /// The probe.
    public var probe: ProbeOutcome
    /// Whether the launch record file exists.
    public var launchRecord: Bool
    /// Whether the probe was read right after a registration in the same action.
    public var justRegistered: Bool

    /// Creates the input.
    public init(
        rootOverride: Bool, probe: ProbeOutcome, launchRecord: Bool, justRegistered: Bool
    ) {
        self.rootOverride = rootOverride
        self.probe = probe
        self.launchRecord = launchRecord
        self.justRegistered = justRegistered
    }
}

/// The state of the agent with its reason and the next step.
nonisolated public struct AgentAssessment: Sendable, Equatable, Hashable {
    /// The state.
    public var state: BackgroundAgentState
    /// Why the state is unavailable, or `nil` for every other state.
    public var reason: UnavailableReason?
    /// The next step.
    public var recovery: RecoveryStep

    /// Creates the assessment.
    public init(
        state: BackgroundAgentState, reason: UnavailableReason? = nil, recovery: RecoveryStep
    ) {
        self.state = state
        self.reason = reason
        self.recovery = recovery
    }
}

/// The pure map from a probe to a state (spec 0015, *State background agent*), the twin
/// of `classify` in `stackly-platform::agent`; both pass `protocols/fixtures/agent/states.json`.
nonisolated public enum BackgroundAgentClassifier {
    /// The state for `input`. The first matching row of the table wins.
    public static func classify(_ input: AgentInput) -> AgentAssessment {
        if input.rootOverride {
            return unavailable(.rootOverride, .manualDaemonForOverride)
        }
        switch input.probe {
        case .unsupported:
            return unavailable(.unsupportedOs, .none)
        case .failed, .notChecked:
            return unavailable(.statusFailed, .checkAgain)
        case .probe(.notBundled):
            return unavailable(.notBundled, .useBundledCli)
        case .probe(.helperMissing):
            return unavailable(.helperMissing, .buildWithXtask)
        case .probe(.registration(let registration, _)):
            return assess(registration, input)
        }
    }

    private static func assess(_ registration: AgentRegistration, _ input: AgentInput)
        -> AgentAssessment
    {
        switch registration {
        case .unrecognized:
            return unavailable(.statusUnrecognized, .checkAgain)
        case .enabled:
            return AgentAssessment(state: .enabled, recovery: .none)
        case .notRegistered:
            return AgentAssessment(state: .notRegistered, recovery: .register)
        case .notFound where input.justRegistered:
            return AgentAssessment(state: .notFound, recovery: .moveOrReinstallApp)
        case .notFound:
            return AgentAssessment(state: .notRegistered, recovery: .register)
        case .requiresApproval where input.launchRecord && input.justRegistered == false:
            return AgentAssessment(state: .disabled, recovery: .enableInLoginItems)
        case .requiresApproval:
            return AgentAssessment(state: .requiresApproval, recovery: .approveInLoginItems)
        }
    }

    private static func unavailable(_ reason: UnavailableReason, _ recovery: RecoveryStep)
        -> AgentAssessment
    {
        AgentAssessment(state: .unavailable, reason: reason, recovery: recovery)
    }
}
