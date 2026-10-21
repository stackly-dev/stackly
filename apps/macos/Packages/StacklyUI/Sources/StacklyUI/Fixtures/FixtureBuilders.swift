#if DEBUG
    import Foundation
    import StacklyClient

    /// Where the fixture daemon keeps its socket.
    public nonisolated enum FixturePaths {
        /// The socket path every fixture shows.
        public static let socket = "/fixture/stackly/stacklyd.sock"
        /// The recovery report path of the `daemon-recovery` scenario.
        public static let recoveryReport = "/fixture/stackly/recovery/report.json"
    }

    nonisolated extension InstanceSummary {
        /// An instance with the defaults of a stopped instance; pass what differs.
        ///
        /// - Parameters:
        ///   - number: The case number, for the ID and the runtime ID.
        ///   - name: The instance name.
        ///   - provider: The provider ID, such as `official/postgresql`.
        ///   - runtimeVersion: The runtime version.
        ///   - port: The port of the one `database` endpoint, or `nil` for none.
        ///   - lifecycle: The lifecycle state.
        ///   - desired: The desired state.
        ///   - readiness: The readiness; unknown by default.
        ///   - health: The health; unknown by default.
        ///   - lastFailure: The last failure.
        ///   - restart: The restart budget; unused by default.
        ///   - blocker: What stops the instance.
        public static func fixture(
            number: UInt8 = 1,
            name: String = "pg17-dev",
            provider: String = "official/postgresql",
            runtimeVersion: String = "17.6",
            port: UInt16? = 5432,
            lifecycle: InstanceLifecycle = .stopped,
            desired: InstanceDesired = .stopped,
            readiness: InstanceReadiness? = nil,
            health: InstanceHealth? = nil,
            lastFailure: InstanceLastFailure? = nil,
            restart: InstanceRestart? = nil,
            blocker: InstanceBlocker? = nil
        ) -> InstanceSummary {
            let observed = FixtureClock.minutesAgo(1)
            let isServing = lifecycle == .running && readiness?.state == .ready
            return InstanceSummary(
                id: FixtureID.uuid(kind: FixtureID.instance, number: number),
                name: name,
                displayName: nil,
                revision: 1,
                software: provider.split(separator: "/").last.map(String.init) ?? provider,
                provider: ProviderPin(
                    name: provider,
                    revision: "2026.10.1",
                    manifestSha256: String(repeating: "3f786850", count: 8)),
                runtimeId: FixtureID.uuid(kind: FixtureID.runtime, number: number),
                runtimeVersion: runtimeVersion,
                lifecycle: lifecycle,
                desired: desired,
                readiness: readiness
                    ?? InstanceReadiness(
                        state: .unknown, reason: "not_running", detail: nil, observedAt: observed),
                health: health
                    ?? InstanceHealth(
                        state: .unknown, reason: "not_running", detail: nil, observedAt: observed),
                statusSeq: 1,
                process: lifecycle == .running
                    ? InstanceProcess(
                        pid: 4000 + UInt32(number), startedAt: FixtureClock.minutesAgo(30),
                        adopted: false, adoptable: nil) : nil,
                lastFailure: lastFailure,
                restart: restart
                    ?? InstanceRestart(
                        attempts: 0, budget: 5, windowSeconds: 60, nextAttemptAt: nil,
                        exhausted: false),
                blocker: blocker,
                endpoints: port.map {
                    [
                        EndpointStatus(
                            name: "database", transport: .tcp, host: "127.0.0.1", port: $0,
                            portMode: .fixed, bound: isServing, socketPath: nil)
                    ]
                },
                leftoverProcesses: nil,
                logsUnavailable: false,
                ownership: .manual,
                initialized: true)
        }
    }

    nonisolated extension InstanceReadiness {
        /// A readiness observed one minute before ``FixtureClock/now``.
        public static func fixture(
            _ state: InstanceReadinessState, reason: String, detail: String? = nil
        ) -> InstanceReadiness {
            InstanceReadiness(
                state: state, reason: reason, detail: detail, observedAt: FixtureClock.minutesAgo(1)
            )
        }
    }

    nonisolated extension InstanceHealth {
        /// A health observed one minute before ``FixtureClock/now``.
        public static func fixture(
            _ state: InstanceHealthState, reason: String, detail: String? = nil
        ) -> InstanceHealth {
            InstanceHealth(
                state: state, reason: reason, detail: detail, observedAt: FixtureClock.minutesAgo(1)
            )
        }
    }

    nonisolated extension OperationSummary {
        /// An operation with the defaults of a queued one; pass what differs.
        ///
        /// - Parameters:
        ///   - number: The case number, for the ID.
        ///   - kind: The operation kind.
        ///   - resource: The resource the operation works on.
        ///   - state: The state.
        ///   - stage: The current stage.
        ///   - progress: The progress.
        ///   - failure: Why it failed.
        ///   - recovery: What recovery needs.
        public static func fixture(
            number: UInt8 = 1,
            kind: OperationKind = .startInstance,
            resource: ResourceRef? = nil,
            state: OperationState = .queued,
            stage: String? = nil,
            progress: OperationProgress = .indeterminate,
            failure: OperationFailureInfo? = nil,
            recovery: RecoveryInfo? = nil
        ) -> OperationSummary {
            let at = FixtureClock.minutesAgo(2)
            return OperationSummary(
                id: FixtureID.uuid(kind: FixtureID.operation, number: number),
                kind: kind,
                resource: resource,
                state: state,
                stage: stage,
                progress: progress,
                waitingOn: nil,
                cancelRequestedAt: nil,
                pointOfNoReturnAt: nil,
                failure: failure,
                recovery: recovery,
                result: nil,
                trigger: OperationTriggerInfo(origin: .user, client: .gui),
                createdAt: at,
                startedAt: state == .queued ? nil : at,
                updatedAt: at,
                revision: 1)
        }
    }

    nonisolated extension ProviderSummary {
        /// A provider with the defaults of an available database; pass what differs.
        public static func fixture(
            provider: String = "official/postgresql",
            displayName: String = "PostgreSQL",
            category: ProviderCategory = .database,
            state: CatalogEntryState = .available,
            stateReason: SupportReason? = nil,
            versions: [String] = ["17.6", "18.1"],
            mark: String = "Pg",
            tile: TileColor = .blue
        ) -> ProviderSummary {
            ProviderSummary(
                provider: provider,
                revision: 1,
                displayName: displayName,
                software: provider.split(separator: "/").last.map(String.init),
                category: category,
                description: nil,
                state: state,
                stateReason: stateReason,
                stateNote: nil,
                providerRevision: "2026.10.1",
                publisher: "Stackly",
                trust: .official,
                versions: versions,
                versionCount: UInt64(versions.count),
                licenses: nil,
                mark: mark,
                tile: tile)
        }
    }

    nonisolated extension CatalogInfo {
        /// The catalog every fixture daemon reports.
        public static let fixture = CatalogInfo(
            revision: 1,
            publishedAt: FixtureClock.daysAgo(3),
            loadedAt: FixtureClock.minutesAgo(5),
            contentSha256: String(repeating: "a1b2c3d4", count: 8),
            source: .bundled,
            trust: .official,
            host: HostTarget(os: .macos, arch: .aarch64),
            outdated: false,
            invalidCount: 0)
    }

    nonisolated extension HandshakeAccepted {
        /// The handshake every connected fixture daemon answered.
        public static let fixture = HandshakeAccepted(
            protocol: FixtureContract.protocolVersion,
            serverInstanceId: FixtureID.uuid(kind: FixtureID.daemon, number: 1),
            daemon: DaemonIdentity(version: "0.0.0-fixture", pid: 4242),
            contractDigest: FixtureContract.digest,
            contractDrift: false,
            limits: Limits(
                maxFrameBytes: 1_048_576, maxLogBatchBytes: 65_536, maxJsonDepth: 32,
                maxInFlightRequests: 32, maxSubscriptions: 8, eventBufferLen: 1024,
                eventReplayLen: 4096),
            head: EventCursor(
                serverInstanceId: FixtureID.uuid(kind: FixtureID.daemon, number: 1), sequence: 0))
    }

    nonisolated extension DaemonInfo {
        /// The daemon facts every connected fixture daemon reports.
        public static let fixture = DaemonInfo(
            version: "0.0.0-fixture",
            protocol: FixtureContract.protocolVersion,
            contractDigest: FixtureContract.digest,
            serverInstanceId: FixtureID.uuid(kind: FixtureID.daemon, number: 1),
            pid: 4242,
            uid: 501,
            startedAt: FixtureClock.minutesAgo(120),
            roots: DaemonRoots(
                data: "/fixture/stackly/data", logs: "/fixture/stackly/logs",
                cache: "/fixture/stackly/cache", runtime: "/fixture/stackly/run",
                rootOverride: false),
            socketPath: FixturePaths.socket,
            registrySchema: 3,
            secretStore: nil)
    }

    nonisolated extension DaemonHealth {
        /// A daemon health with `status`, healthy subsystems unless `degraded` names one, and
        /// zero counters.
        ///
        /// - Parameters:
        ///   - status: The overall status.
        ///   - degraded: The name of one subsystem to report `degraded`.
        ///   - recovery: Registry recovery, when the daemon is in recovery mode.
        public static func fixture(
            _ status: DaemonHealthStatus = .ready,
            degraded: String? = nil,
            recovery: RegistryRecovery? = nil
        ) -> DaemonHealth {
            let names = ["registry", "operations", "supervisor"]
            return DaemonHealth(
                status: status,
                subsystems: names.map {
                    SubsystemHealth(
                        name: $0, status: $0 == degraded ? .degraded : .ok,
                        detail: $0 == degraded ? "the registry answers slowly" : nil)
                },
                counters: DaemonCounters(
                    connectionsOpen: 1, connectionsTotal: 1, connectionsRejected: 0,
                    framesRejected: 0, eventsPublished: 0, eventsDropped: 0, logLinesDropped: 0,
                    socketRebinds: 0),
                recovery: recovery)
        }
    }

    nonisolated extension ErrorBody {
        /// `resource.not_found` for the resource `kind` and `id`.
        public static func fixtureNotFound(_ kind: ResourceKind, id: String) -> ErrorBody {
            ErrorBody(
                code: "resource.not_found", category: .notFound,
                message: "no \(kind.rawValue) with this id", retryable: false,
                resource: ResourceRef(kind: kind, id: id), fields: nil, hint: nil, details: nil,
                operationId: nil, rule: nil, items: nil)
        }
    }
#endif
