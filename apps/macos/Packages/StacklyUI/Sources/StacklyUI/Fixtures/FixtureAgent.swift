#if DEBUG
    import Foundation

    /// What the fake agent answers: a probe, or an error (spec 0068, *Skenario agent*).
    public nonisolated enum FixtureAgentAnswer: Sendable, Equatable {
        /// The OS answers with this probe.
        case probe(AgentProbe)
        /// The call fails with this error.
        case error(AgentActionError)

        /// The answer as the fake controller takes it.
        var result: Result<AgentProbe, AgentActionError> {
            switch self {
            case .probe(let probe): .success(probe)
            case .error(let error): .failure(error)
            }
        }
    }

    /// The background agent of a scenario: what `status()` answers, what `register()` and
    /// `relaunch()` answer (and `status()` after them), and the launch record. The shell
    /// fixture plays it through ``FakeBackgroundAgentController`` and
    /// ``FakeLaunchRecordReader``, so it never calls `SMAppService` or reads a file.
    public nonisolated struct FixtureAgent: Sendable, Equatable {
        /// The answer of `status()`.
        public var status: FixtureAgentAnswer
        /// The answer of `register()` and `relaunch()`, and of `status()` after them.
        public var afterRegister: FixtureAgentAnswer
        /// When launchd last started the daemon; `nil` means there is no launch record.
        public var launchedAt: Date?

        /// Creates the agent.
        public init(
            status: FixtureAgentAnswer, afterRegister: FixtureAgentAnswer, launchedAt: Date? = nil
        ) {
            self.status = status
            self.afterRegister = afterRegister
            self.launchedAt = launchedAt
        }

        /// The app bundle every fixture agent names.
        public static let bundle = "/fixture/Applications/Stackly.app"

        private static func registration(_ registration: AgentRegistration) -> FixtureAgentAnswer {
            .probe(.registration(registration, bundle: bundle))
        }

        /// A new install: `notFound`, so the state is `not_registered`; Turn On gives
        /// `requiresApproval`. Every scenario has it except the agent scenarios.
        public static let fresh = FixtureAgent(
            status: registration(.notFound), afterRegister: registration(.requiresApproval))

        /// `requiresApproval` without a launch record.
        public static let requiresApproval = FixtureAgent(
            status: registration(.requiresApproval),
            afterRegister: registration(.requiresApproval))

        /// `requiresApproval` with a launch record: turned off in Login Items.
        public static let disabled = FixtureAgent(
            status: registration(.requiresApproval),
            afterRegister: registration(.requiresApproval), launchedAt: FixtureClock.minutesAgo(30))

        /// `enabled` with a launch record.
        public static let enabled = FixtureAgent(
            status: registration(.enabled), afterRegister: registration(.enabled),
            launchedAt: FixtureClock.minutesAgo(30))

        /// `notFound` before Turn On and after it.
        public static let notFound = FixtureAgent(
            status: registration(.notFound), afterRegister: registration(.notFound))

        /// A status call that times out.
        public static let unavailable = FixtureAgent(
            status: .error(.timedOut(op: .status)), afterRegister: registration(.requiresApproval))
    }
#endif
