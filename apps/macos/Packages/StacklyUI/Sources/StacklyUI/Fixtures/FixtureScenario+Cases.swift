#if DEBUG
    import Foundation
    import StacklyClient

    nonisolated extension FixtureScenario {
        /// The instance cases of `connected-mixed`, numbered as in spec 0068.
        public static let instanceCases: [InstanceSummary] = [
            .fixture(
                number: 1, name: "pg17-dev", lifecycle: .running, desired: .running,
                readiness: .fixture(.ready, reason: "probe_ok"),
                health: .fixture(.healthy, reason: "probe_ok")),
            .fixture(
                number: 2, name: "orders-db", runtimeVersion: "18.1", port: 5433,
                lifecycle: .running, desired: .running,
                readiness: .fixture(.notReady, reason: "awaiting_probe")),
            .fixture(
                number: 3, name: "cache", provider: "official/valkey", runtimeVersion: "8.1.4",
                port: 6379, lifecycle: .stopping, desired: .stopped),
            .fixture(
                number: 4, name: "mysql-erp", provider: "official/mysql", runtimeVersion: "8.4.6",
                port: 3306),
            .fixture(
                number: 5, name: "queue", provider: "official/nats", runtimeVersion: "2.11.9",
                port: 4222, lifecycle: .failed, desired: .running,
                lastFailure: InstanceLastFailure(
                    code: "crash_loop", at: FixtureClock.minutesAgo(3), exitCode: nil, signal: nil,
                    excerpt: [
                        "[ERR] listener failed: address already in use",
                        "[FTL] server exited",
                    ]),
                restart: InstanceRestart(
                    attempts: 5, budget: 5, windowSeconds: 60, nextAttemptAt: nil, exhausted: true)),
            .fixture(
                number: 6, name: "analytics", provider: "official/clickhouse",
                runtimeVersion: "25.8.3", port: 8123, lifecycle: .failed, desired: .running,
                lastFailure: InstanceLastFailure(
                    code: "process_exited", at: FixtureClock.minutesAgo(3), exitCode: 1,
                    signal: nil, excerpt: nil)),
            .fixture(
                number: 7, name: "legacy-pg", runtimeVersion: "16.10", port: 5434,
                blocker: .dataInUse(
                    IdentityBlocker(
                        evidence: IdentityEvidence(
                            checks: [
                                IdentityCheckEntry(name: .bootMatches, result: .passed),
                                IdentityCheckEntry(name: .pidExists, result: .failed),
                                IdentityCheckEntry(name: .holdsLock, result: .notChecked),
                            ],
                            pid: 4242, lock: .held, stopped: true, error: nil),
                        observedAt: FixtureClock.minutesAgo(1)))),
            .fixture(
                number: 8, name: "ledger", port: 5435, lifecycle: .unknown, desired: .running,
                blocker: .identityUnproven(
                    IdentityBlocker(
                        evidence: IdentityEvidence(
                            checks: [
                                IdentityCheckEntry(name: .bootMatches, result: .passed),
                                IdentityCheckEntry(name: .pidExists, result: .passed),
                                IdentityCheckEntry(name: .startMatches, result: .failed),
                            ],
                            pid: 4300, lock: nil, stopped: false, error: nil),
                        observedAt: FixtureClock.minutesAgo(1)))),
            .fixture(
                number: 9, name: "billing-db", port: 5436, desired: .running,
                blocker: .needsSecret(
                    NeedsSecretBlocker(
                        references: [
                            NeedsSecretItem(
                                reference:
                                    "local:instance/\(FixtureID.uuid(kind: FixtureID.instance, number: 9).uuidString.lowercased())/superuser_password",
                                reason: .missing, generateAllowed: true)
                        ],
                        observedAt: FixtureClock.minutesAgo(1)))),
            .fixture(
                number: 10, name: "search", runtimeVersion: "18.1", port: 5437,
                lifecycle: .running, desired: .running,
                readiness: .fixture(.notReady, reason: "probe_failed", detail: "connection refused")
            ),
            .fixture(
                number: 11, name: "metrics", port: 5438, lifecycle: .running, desired: .running,
                readiness: .fixture(.ready, reason: "probe_ok"),
                health: .fixture(.degraded, reason: "slow_queries")),
        ]

        /// The operation cases of `connected-mixed`, numbered as in spec 0068.
        public static let operationCases: [OperationSummary] = [
            .fixture(
                number: 1, kind: .installRuntime,
                resource: instanceCases[1].runtimeReference, state: .running, stage: "download",
                progress: .counted(CountedProgress(done: 42, total: 100))),
            .fixture(
                number: 2, kind: .startInstance, resource: instanceCases[3].reference,
                state: .queued),
            .fixture(
                number: 3, kind: .stopInstance, resource: instanceCases[5].reference,
                state: .failed,
                failure: OperationFailureInfo(
                    reason: "stop_timed_out", retryable: true,
                    error: ErrorBody(
                        code: "supervisor.stop_timed_out", category: .timeout,
                        message: "the process did not stop in time", retryable: true,
                        resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
                        rule: nil, items: nil))),
            .fixture(
                number: 4, kind: .createInstance, state: .needsRecovery,
                recovery: RecoveryInfo(
                    hint: .resume, cause: .interrupted, error: nil, actions: [.resume, .abandon])),
        ]

        /// The provider cases of `connected-mixed`.
        public static let providerCases: [ProviderSummary] = [
            .fixture(),
            .fixture(
                provider: "official/valkey", displayName: "Valkey", category: .cache,
                state: .planned, stateReason: .providerPlanned, versions: ["8.1.4"], mark: "Vk",
                tile: .red),
            .fixture(
                provider: "official/clickhouse", displayName: "ClickHouse", category: .analytics,
                state: .unsupported, stateReason: .noBuildForHost, versions: [], mark: "Ch",
                tile: .yellow),
        ]

        // MARK: - Connection values

        static let unreachableError = ClientErrorCatalog.connectionRefused(
            path: FixturePaths.socket)

        static let majorMismatchError = ErrorBody(
            code: "protocol.major_mismatch", category: .unsupported,
            message: "the client speaks another protocol major", retryable: false, resource: nil,
            fields: nil, hint: nil,
            details: [
                ErrorDetail(key: "serverMajor", value: "2"),
                ErrorDetail(key: "serverMinor", value: "0"),
            ], operationId: nil, rule: nil, items: nil)

        static let handshakeRejectedError = ErrorBody(
            code: "request.invalid_params", category: .invalidInput,
            message: "the params fail to decode or to validate", retryable: false, resource: nil,
            fields: nil, hint: nil,
            details: [ErrorDetail(key: "method", value: "daemon.handshake")],
            operationId: nil, rule: nil, items: nil)

        static let untrustedError = ClientErrorCatalog.endpointUntrusted(
            path: FixturePaths.socket, reason: .groupOrOtherAccess)

        static let recovery = RegistryRecovery(
            reason: "schema_newer", reportPath: FixturePaths.recoveryReport,
            detectedAt: FixtureClock.minutesAgo(5), appSchemaVersion: 3, dbSchemaVersion: 4)

        static func connected(_ health: DaemonHealth) -> ConnectionState {
            .connected(
                ConnectedDaemon(
                    accepted: .fixture, info: .fixture, health: health,
                    connectedAt: FixtureClock.now))
        }

        static let unreachable = ConnectionState.unreachable(
            unreachableError, since: FixtureClock.now,
            nextAttempt: FixtureClock.now.addingTimeInterval(5))

        private static let blockedNextAttempt = FixtureClock.now.addingTimeInterval(30)

        // MARK: - Catalog

        /// Builds the scenario named `id`.
        static func build(_ id: ID) -> FixtureScenario {
            let mixed = (instanceCases, operationCases, providerCases)
            switch id {
            case .connectedEmpty:
                return connectedScenario(
                    id, health: .fixture(), presence: .connected(health: .ready))
            case .connectedMixed:
                return connectedScenario(
                    id, health: .fixture(), data: mixed, presence: .connected(health: .ready))
            case .daemonDegraded:
                return connectedScenario(
                    id, health: .fixture(.degraded, degraded: "registry"),
                    data: mixed, presence: .connected(health: .degraded))
            case .daemonHealthUnrecognized:
                return connectedScenario(
                    id, health: .fixture(.unrecognized), data: mixed,
                    presence: .connected(health: .unknown))
            case .daemonRefreshing:
                return connectedScenario(
                    id, health: .fixture(), holdSnapshot: true, presence: .refreshing)
            case .daemonStarting:
                return connectedScenario(id, health: .fixture(.starting), presence: .starting)
            case .daemonStopping:
                return connectedScenario(
                    id, health: .fixture(.stopping), data: mixed, presence: .stopping)
            case .daemonConnecting:
                return FixtureScenario(
                    id: id, connection: [.state(.connecting(attempt: 1))],
                    expectedPresence: .connecting(slow: true))
            case .daemonOffline:
                return connectedScenario(
                    id, health: .fixture(), data: mixed, afterSnapshot: unreachable,
                    presence: .unreachable(lastKnownAt: FixtureClock.now))
            case .daemonUnreachable:
                return FixtureScenario(
                    id: id, connection: [.state(unreachable)],
                    expectedPresence: .unreachable(lastKnownAt: nil))
            case .daemonIncompatibleMajor:
                let reason = IncompatibleReason.majorMismatch(
                    server: ProtocolVersion(major: 2, minor: 0), error: majorMismatchError)
                return FixtureScenario(
                    id: id,
                    connection: [
                        .state(
                            .incompatible(
                                reason, since: FixtureClock.now, nextAttempt: blockedNextAttempt))
                    ],
                    expectedPresence: .incompatible(reason))
            case .daemonIncompatibleRejected:
                let reason = IncompatibleReason.handshakeRejected(handshakeRejectedError)
                return FixtureScenario(
                    id: id,
                    connection: [
                        .state(
                            .incompatible(
                                reason, since: FixtureClock.now, nextAttempt: blockedNextAttempt))
                    ],
                    expectedPresence: .incompatible(reason))
            case .daemonUntrusted:
                return FixtureScenario(
                    id: id,
                    connection: [
                        .state(
                            .untrusted(
                                untrustedError, since: FixtureClock.now,
                                nextAttempt: blockedNextAttempt))
                    ],
                    expectedPresence: .untrusted(
                        reason: UntrustedReason.groupOrOtherAccess.rawValue,
                        path: FixturePaths.socket))
            case .daemonRecovery:
                return connectedScenario(
                    id, health: .fixture(.ready, recovery: recovery), presence: .recovery)
            case .daemonMisconfigured:
                return FixtureScenario(
                    id: id, connection: [.state(.misconfigured(.rootOverride(.empty)))],
                    expectedPresence: .misconfigured(.rootOverride(.empty)))
            case .onboarding:
                return FixtureScenario(
                    id: id, connection: [.state(connected(.fixture()))],
                    handshake: .fixture, daemonInfo: .fixture, health: .fixture(),
                    onboardingFinished: false, expectedPresence: nil)
            }
        }

        private static func connectedScenario(
            _ id: ID,
            health: DaemonHealth,
            data: ([InstanceSummary], [OperationSummary], [ProviderSummary]) = ([], [], []),
            holdSnapshot: Bool = false,
            afterSnapshot: ConnectionState? = nil,
            presence: DaemonPresence
        ) -> FixtureScenario {
            var steps: [FixtureConnectionStep] = [.state(connected(health))]
            if holdSnapshot { steps.append(.holdSnapshot) }
            if let afterSnapshot { steps.append(.afterSnapshot(afterSnapshot)) }
            return FixtureScenario(
                id: id, connection: steps, handshake: .fixture, daemonInfo: .fixture,
                health: health, instances: data.0, operations: data.1, providers: data.2,
                expectedPresence: presence)
        }
    }

    nonisolated extension InstanceSummary {
        /// The reference to this instance.
        fileprivate var reference: ResourceRef {
            ResourceRef(kind: .instance, id: id.uuidString.lowercased())
        }

        /// The reference to this instance's runtime.
        fileprivate var runtimeReference: ResourceRef {
            ResourceRef(kind: .runtime, id: runtimeId.uuidString.lowercased())
        }
    }
#endif
