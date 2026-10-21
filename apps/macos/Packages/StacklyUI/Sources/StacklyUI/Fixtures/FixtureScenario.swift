#if DEBUG
    import Foundation
    import StacklyClient

    /// One step of a scenario's connection script.
    public nonisolated enum FixtureConnectionStep: Sendable, Equatable {
        /// The link reports this status at once.
        case state(ConnectionState)
        /// The link reports this status after it answered the `list` call of every store
        /// the shell syncs.
        case afterSnapshot(ConnectionState)
        /// The `list` call of a store is never answered, so the snapshot never ends.
        case holdSnapshot
    }

    /// What a scenario answers to a method outside the ones it answers from its data.
    public nonisolated enum FixtureReply: Sendable, Equatable {
        /// The call succeeds with this reply.
        case reply(Reply)
        /// The call fails with this error.
        case error(ClientError)
    }

    /// A whole daemon world for the app and the tests: what the link reports and what the
    /// daemon answers, built only from the generated contract types (spec 0068).
    ///
    /// The catalog of scenarios is ``all``; each is named by an ``ID`` that is also the
    /// value of `STACKLY_UI_SCENARIO`.
    public nonisolated struct FixtureScenario: Sendable {
        /// The name of a scenario, in kebab case.
        public enum ID: String, CaseIterable, Sendable {
            /// Connected to a ready daemon with no instances and no operations.
            case connectedEmpty = "connected-empty"
            /// Connected to a ready daemon with every instance, operation, and provider case.
            case connectedMixed = "connected-mixed"
            /// Connected to a degraded daemon.
            case daemonDegraded = "daemon-degraded"
            /// Connected to a daemon whose health status this app does not know.
            case daemonHealthUnrecognized = "daemon-health-unrecognized"
            /// Connected while the stores are still loading and never finish.
            case daemonRefreshing = "daemon-refreshing"
            /// Connected to a daemon that is starting.
            case daemonStarting = "daemon-starting"
            /// Connected to a daemon that is stopping.
            case daemonStopping = "daemon-stopping"
            /// The first attempt, which never ends.
            case daemonConnecting = "daemon-connecting"
            /// Connected first, then the daemon becomes unreachable.
            case daemonOffline = "daemon-offline"
            /// Unreachable from the start.
            case daemonUnreachable = "daemon-unreachable"
            /// A daemon of another protocol major.
            case daemonIncompatibleMajor = "daemon-incompatible-major"
            /// A daemon that refused the handshake.
            case daemonIncompatibleRejected = "daemon-incompatible-rejected"
            /// A socket that failed a security check.
            case daemonUntrusted = "daemon-untrusted"
            /// Connected to a daemon whose registry is in recovery mode.
            case daemonRecovery = "daemon-recovery"
            /// A client configuration that stops it.
            case daemonMisconfigured = "daemon-misconfigured"
            /// A ready daemon with no instances, before onboarding finished.
            case onboarding
        }

        /// The scenario's name.
        public let id: ID
        /// What the link reports, in order.
        public let connection: [FixtureConnectionStep]
        /// The handshake reply, or `nil` when no connection opens.
        public let handshake: HandshakeAccepted?
        /// The `daemon.get_info` reply.
        public let daemonInfo: DaemonInfo?
        /// The `daemon.get_health` reply.
        public let health: DaemonHealth?
        /// The instances `instance.list` returns.
        public let instances: [InstanceSummary]
        /// The operations `operation.list` returns.
        public let operations: [OperationSummary]
        /// The providers `catalog.list_providers` returns.
        public let providers: [ProviderSummary]
        /// Answers to other methods, by method name such as `secret.list`.
        public let replies: [String: FixtureReply]
        /// Whether onboarding is finished.
        public let onboardingFinished: Bool
        /// The presence the shell settles in, or `nil` when the scenario shows onboarding.
        public let expectedPresence: DaemonPresence?

        /// Creates a scenario from its parts.
        public init(
            id: ID,
            connection: [FixtureConnectionStep],
            handshake: HandshakeAccepted? = nil,
            daemonInfo: DaemonInfo? = nil,
            health: DaemonHealth? = nil,
            instances: [InstanceSummary] = [],
            operations: [OperationSummary] = [],
            providers: [ProviderSummary] = [],
            replies: [String: FixtureReply] = [:],
            onboardingFinished: Bool = true,
            expectedPresence: DaemonPresence?
        ) {
            self.id = id
            self.connection = connection
            self.handshake = handshake
            self.daemonInfo = daemonInfo
            self.health = health
            self.instances = instances
            self.operations = operations
            self.providers = providers
            self.replies = replies
            self.onboardingFinished = onboardingFinished
            self.expectedPresence = expectedPresence
        }

        /// Every scenario, in the order of ``ID``.
        public static let all: [FixtureScenario] = ID.allCases.map(FixtureScenario.init)

        /// The scenario named `id`.
        public init(_ id: ID) {
            self = Self.build(id)
        }

        /// Whether the connection script ends in a connected daemon.
        public var connects: Bool {
            handshake != nil
        }
    }
#endif
