import StacklyClient

/// What the Autostart row needs to know besides the instance (spec 0031, *Copy*, AC-19): how
/// this daemon run rotated into its login session, and whether the background agent can start
/// the daemon at login.
nonisolated public struct AutostartContext: Sendable, Equatable {
    /// How the daemon run rotated into its session, or `nil` when the daemon reports no
    /// session (a daemon launchd did not start).
    public var rotation: SessionRotationKind?
    /// The state of the background agent, or `nil` when it was not read.
    public var agent: BackgroundAgentState?
    /// Whether the `STACKLY_ROOT` override is active, which has no agent.
    public var rootOverride: Bool

    /// Creates a context.
    /// - Parameters:
    ///   - rotation: How the daemon run rotated into its session, or `nil`.
    ///   - agent: The state of the background agent, or `nil`.
    ///   - rootOverride: Whether the root override is active.
    public init(
        rotation: SessionRotationKind? = nil, agent: BackgroundAgentState? = nil,
        rootOverride: Bool = false
    ) {
        self.rotation = rotation
        self.agent = agent
        self.rootOverride = rootOverride
    }

    /// A context that knows nothing: no session, no agent.
    public static let unknown = AutostartContext()
}

/// The words of the Autostart row (spec 0031, *Copy*).
nonisolated public enum AutostartText {
    /// "Won’t start at login: the background agent is off."
    public static let agentOff = "Won’t start at login: the background agent is off."
    /// "Won’t start at login: the background agent needs approval in Login Items."
    public static let agentNeedsApproval =
        "Won’t start at login: the background agent needs approval in Login Items."
    /// "Stopped for this session. Starts at your next login."
    public static let suppressed = "Stopped for this session. Starts at your next login."
    /// "Starts at your next login."
    public static let nextLogin = "Starts at your next login."

    /// Whether the agent keeps an instance that starts at login from starting: the daemon
    /// runs in a login session on the default root and the agent is not `enabled`.
    /// - Parameter context: The session and the agent.
    public static func agentBlocks(_ context: AutostartContext) -> BackgroundAgentState? {
        guard context.rotation != nil, context.rootOverride == false,
            let agent = context.agent
        else { return nil }
        switch agent {
        case .requiresApproval, .disabled, .notRegistered, .notFound: return agent
        case .enabled, .unavailable: return nil
        }
    }

    /// The note under the toggle, or `nil` when there is none.
    /// - Parameters:
    ///   - autostart: The policy and the lease of this session.
    ///   - desired: What the person wants of the instance.
    ///   - context: The session and the agent.
    public static func note(
        autostart: InstanceAutostart, desired: InstanceDesired, context: AutostartContext
    ) -> String? {
        guard autostart.policy == .atLogin else { return nil }
        if let agent = agentBlocks(context) {
            return agent == .requiresApproval ? agentNeedsApproval : agentOff
        }
        switch autostart.session {
        case .suppressed:
            return desired == .stopped ? suppressed : nil
        case .none:
            return context.rotation == .new ? nil : nextLogin
        default:
            return nil
        }
    }
}

extension InstanceSummary {
    /// The startup policy and the lease of this session; an older daemon sends none, which
    /// reads as `manual` and `none`.
    public var startup: InstanceAutostart {
        autostart ?? InstanceAutostart(policy: .manual, session: .none)
    }
}
