#if DEBUG
    import Foundation
    import StacklyClient

    /// The failure banners of spec 0033 (*State banner*, F1 to F7), as fixture cases of spec
    /// 0068: each case is one instance and the operations `operation.list { resource }`
    /// answers for it, built only from the generated contract types. The port cases extend
    /// ``FixtureStartConflict``: `portInUse` is its instance with the start that failed, and
    /// `portWithoutOperation` is the same instance as the summary alone shows it.
    ///
    /// Like ``FixtureStartConflict`` the cases join no scenario by themselves, so the instance
    /// counts of the existing scenarios stay; a rig adds the instance and its operations.
    /// No excerpt, message, or hint holds a secret.
    public nonisolated enum FixtureFailureBanner: String, CaseIterable, Sendable {
        /// F1: a restart waits 4 seconds, the second respawn of five.
        case restarting
        /// F2: initialization failed before the data folder was ready.
        case initFailed
        /// F3: the restart budget ran out during the start the person asked for.
        case crashLoop
        /// F4 on a `failed` instance: another process holds the port, Use Port 5439.
        case portInUse
        /// F4 from the summary alone (no operation read yet, or after a daemon restart):
        /// Change Port….
        case portWithoutOperation
        /// F4 on a `stopped` instance: pg17-dev owns the port, Use Port 5441.
        case portConflict
        /// F4 on a `stopped` instance: macOS refused the address, no free port, Change Port….
        case portUnavailable
        /// F4 after the ports changed: the note and no port button.
        case portsChanged
        /// F5: another process holds the data lock.
        case dataInUse
        /// F6: the process was stopped by a signal.
        case failed
        /// F7: the daemon refused the start before its journal, the instance stays stopped.
        case startRefused

        /// The case number of the instance.
        public var number: UInt8 {
            switch self {
            case .portInUse, .portWithoutOperation: FixtureStartConflict.number
            case .restarting: 13
            case .initFailed: 14
            case .crashLoop: 15
            case .portConflict: 16
            case .portUnavailable: 17
            case .portsChanged: 18
            case .dataInUse: 19
            case .failed: 20
            case .startRefused: 21
            }
        }

        /// The instance ID.
        public var id: UUID { FixtureID.uuid(kind: FixtureID.instance, number: number) }

        /// The instance.
        public var instance: InstanceSummary {
            switch self {
            case .restarting:
                return .fixture(
                    number: number, name: "worker", provider: "official/valkey",
                    runtimeVersion: "8.1.4", port: 6380, lifecycle: .recovering, desired: .running,
                    lastFailure: InstanceLastFailure(
                        code: "process_exited", at: FixtureClock.secondsAgo(2), exitCode: 3,
                        signal: nil,
                        excerpt: [
                            "# Can't load module from /opt/modules/search.so",
                            "# Module search failed to load",
                        ]),
                    restart: InstanceRestart(
                        attempts: 2, budget: 5, windowSeconds: 300,
                        nextAttemptAt: FixtureClock.now.addingTimeInterval(4), exhausted: false),
                    restartTotal: 2)
            case .initFailed:
                return InstanceSummary.fixture(
                    number: number, name: "fresh-db", runtimeVersion: "18.1", port: 5440,
                    lifecycle: .failed, desired: .running,
                    lastFailure: InstanceLastFailure(
                        code: "init_step_failed", at: FixtureClock.minutesAgo(1), exitCode: 1,
                        signal: nil,
                        excerpt: [
                            "initdb: error: directory \"data\" exists but is not empty",
                            "initdb: hint: Remove the directory or run initdb with another one.",
                        ])
                )
                .uninitialized
            case .crashLoop:
                return .fixture(
                    number: number, name: "events", provider: "official/nats",
                    runtimeVersion: "2.11.9", port: 4223, lifecycle: .failed, desired: .running,
                    lastFailure: InstanceLastFailure(
                        code: "crash_loop", at: Self.failedAt, exitCode: 3, signal: nil,
                        excerpt: [
                            "[ERR] Error parsing config file: unknown field \"jetstream_max\"",
                            "[FTL] Server exiting",
                        ]),
                    restart: InstanceRestart(
                        attempts: 6, budget: 5, windowSeconds: 300, nextAttemptAt: nil,
                        exhausted: true),
                    restartTotal: 5)
            case .portInUse, .portWithoutOperation:
                return FixtureStartConflict.instance
            case .portConflict:
                return .fixture(number: number, name: "reports-db", port: 5432)
            case .portUnavailable:
                return .fixture(number: number, name: "sandbox-db", port: 5442)
            case .portsChanged:
                return .fixture(
                    number: number, name: "audit-db", port: 5444, lifecycle: .failed,
                    desired: .running,
                    lastFailure: InstanceLastFailure(
                        code: "endpoint_in_use", at: FixtureClock.minutesAgo(3), exitCode: 1,
                        signal: nil,
                        excerpt: [
                            "FATAL: could not bind IPv4 address \"127.0.0.1\": Address already in use"
                        ]))
            case .dataInUse:
                return .fixture(
                    number: number, name: "archive-db", runtimeVersion: "16.10", port: 5445,
                    lifecycle: .failed, desired: .running,
                    lastFailure: InstanceLastFailure(
                        code: "data_in_use", at: Self.failedAt, exitCode: 1, signal: nil,
                        excerpt: [
                            "FATAL: lock file \"postmaster.pid\" already exists",
                            "HINT: Is another postmaster (PID 4321) running in data directory?",
                        ]))
            case .failed:
                return .fixture(
                    number: number, name: "warehouse", provider: "official/clickhouse",
                    runtimeVersion: "25.8.3", port: 8124, lifecycle: .failed, desired: .running,
                    lastFailure: InstanceLastFailure(
                        code: "process_signaled", at: Self.failedAt, exitCode: nil, signal: 9,
                        excerpt: ["<Fatal> Application: Child process was terminated by signal 9."]
                    ))
            case .startRefused:
                return .fixture(number: number, name: "inventory-db", port: 5446)
            }
        }

        /// What `operation.list { resource }` answers for the instance, newest first.
        public var operations: [OperationSummary] {
            let resource = ResourceRef(kind: .instance, id: id.uuidString.lowercased())
            func failedStart(_ op: UInt8, _ error: ErrorBody, at: Date = Self.endedAt)
                -> OperationSummary
            {
                Self.operation(
                    op, kind: .startInstance, resource: resource, state: .failed,
                    failure: OperationFailureInfo(
                        reason: "step_failed", retryable: true,
                        error: error.withOperation(
                            FixtureID.uuid(kind: FixtureID.operation, number: op))),
                    endedAt: at)
            }
            switch self {
            case .restarting, .initFailed, .failed, .portWithoutOperation:
                return []
            case .crashLoop:
                return [
                    failedStart(
                        41,
                        Self.error(
                            "supervisor.retry_budget_exhausted", .failed,
                            "the instance crashed 6 times in 300 seconds",
                            hint:
                                "It kept crashing, so Stackly stopped restarting it. Read its logs, fix the cause, then start it again.",
                            details: [("attempts", "6"), ("lastExitCode", "3")]))
                ]
            case .portInUse:
                return [
                    failedStart(
                        42,
                        Self.error(
                            "supervisor.endpoint_in_use", .conflict,
                            "port 5432 of endpoint database is in use by another process",
                            hint: Self.inUseHint,
                            details: [
                                ("endpoint", "database"), ("address", "127.0.0.1"),
                                ("port", "5432"), ("suggestedPort", "5439"), ("omitted", "0"),
                            ]))
                ]
            case .portConflict:
                return [
                    failedStart(
                        43,
                        Self.error(
                            "supervisor.endpoint_conflict", .conflict,
                            "port 5432 of endpoint database belongs to another instance",
                            hint:
                                "Stackly never moves a fixed port on its own. Pick another port, or stop that instance.",
                            details: [
                                ("endpoint", "database"), ("port", "5432"),
                                (
                                    "otherInstance",
                                    FixtureID.uuid(kind: FixtureID.instance, number: 1)
                                        .uuidString.lowercased()
                                ),
                                ("suggestedPort", "5441"), ("omitted", "0"),
                            ]))
                ]
            case .portUnavailable:
                return [
                    failedStart(
                        44,
                        Self.error(
                            "supervisor.endpoint_unavailable", .unavailable,
                            "port 5442 of endpoint database is not available",
                            hint: "Pick another port for this endpoint.",
                            details: [
                                ("endpoint", "database"), ("address", "127.0.0.2"),
                                ("port", "5442"), ("reason", "address_not_available"),
                                ("omitted", "0"),
                            ]))
                ]
            case .portsChanged:
                return [
                    Self.operation(
                        46, kind: .changeEndpoints, resource: resource, state: .succeeded,
                        failure: nil, endedAt: FixtureClock.minutesAgo(1)),
                    failedStart(
                        45,
                        Self.error(
                            "supervisor.endpoint_in_use", .conflict,
                            "port 5444 of endpoint database is in use by another process",
                            hint: Self.inUseHint,
                            details: [
                                ("endpoint", "database"), ("address", "127.0.0.1"),
                                ("port", "5444"), ("suggestedPort", "5447"), ("omitted", "0"),
                            ]), at: FixtureClock.minutesAgo(3).addingTimeInterval(5)),
                ]
            case .dataInUse:
                return [
                    failedStart(
                        47,
                        Self.error(
                            "supervisor.data_in_use", .conflict,
                            "another process holds the instance lock",
                            hint:
                                "Stackly never removes the lock on its own. Stop that process, then start it again.",
                            details: [
                                (
                                    "lockPath",
                                    "\(FixtureDetail.home)/Library/Application Support/Stackly/instances/archive-db/run/instance.lock"
                                ),
                                ("pid", "4321"),
                            ]))
                ]
            case .startRefused:
                return [
                    failedStart(
                        48,
                        Self.error(
                            "supervisor.runtime_unavailable", .conflict,
                            "runtime postgresql 17.6 is not installed", hint: nil,
                            details: [
                                (
                                    "runtimeId",
                                    FixtureID.uuid(kind: FixtureID.runtime, number: 21)
                                        .uuidString.lowercased()
                                ),
                                ("state", "missing"),
                            ]))
                ]
            }
        }

        /// Every case's instance, for a rig that shows them all.
        public static var instances: [InstanceSummary] {
            var seen = Set<UUID>()
            return allCases.map(\.instance).filter { seen.insert($0.id).inserted }
        }

        /// Every case's operations, for a rig that shows them all; `portWithoutOperation` adds
        /// none, so the instance of `portInUse` keeps its operation.
        public static var allOperations: [OperationSummary] {
            allCases.flatMap(\.operations)
        }

        // MARK: - Pieces

        /// When the failure of a `failed` case was recorded.
        static let failedAt = FixtureClock.minutesAgo(1)
        /// When the operation of a case ended: after the failure it caused.
        static let endedAt = FixtureClock.minutesAgo(1).addingTimeInterval(1)

        private static let inUseHint =
            "Stackly never moves a fixed port on its own. Pick another port for this endpoint, or stop the process that holds it."

        private static func error(
            _ code: String, _ category: ErrorCategory, _ message: String, hint: String?,
            details: [(String, String)]
        ) -> ErrorBody {
            ErrorBody(
                code: code, category: category, message: message, retryable: false,
                resource: nil, fields: nil, hint: hint,
                details: details.map { ErrorDetail(key: $0.0, value: $0.1) }, operationId: nil,
                rule: nil, items: nil)
        }

        private static func operation(
            _ number: UInt8, kind: OperationKind, resource: ResourceRef, state: OperationState,
            failure: OperationFailureInfo?, endedAt: Date
        ) -> OperationSummary {
            let createdAt = endedAt.addingTimeInterval(-2)
            return OperationSummary(
                id: FixtureID.uuid(kind: FixtureID.operation, number: number), kind: kind,
                resource: resource, state: state, stage: state == .failed ? "endpoints" : nil,
                progress: .indeterminate, waitingOn: nil, cancelRequestedAt: nil,
                pointOfNoReturnAt: nil, failure: failure, recovery: nil, result: nil,
                trigger: OperationTriggerInfo(origin: .user, client: .gui), createdAt: createdAt,
                startedAt: createdAt, updatedAt: endedAt, revision: 3, cancel: nil, parent: nil)
        }
    }

    nonisolated extension ErrorBody {
        /// The same error with `operationId` set, as an operation's failure carries it.
        fileprivate func withOperation(_ id: UUID) -> ErrorBody {
            ErrorBody(
                code: code, category: category, message: message, retryable: retryable,
                resource: resource, fields: fields, hint: hint, details: details, operationId: id,
                rule: rule, items: items)
        }
    }

    nonisolated extension InstanceSummary {
        /// The same instance whose data folder was never initialized.
        fileprivate var uninitialized: InstanceSummary {
            InstanceSummary(
                id: id, name: name, displayName: displayName, revision: revision,
                software: software, provider: provider, runtimeId: runtimeId,
                runtimeVersion: runtimeVersion, lifecycle: lifecycle, desired: desired,
                readiness: readiness, health: health, statusSeq: statusSeq, process: process,
                lastFailure: lastFailure, restart: restart, restartTotal: restartTotal,
                blocker: blocker, endpoints: endpoints, leftoverProcesses: leftoverProcesses,
                logsUnavailable: logsUnavailable, ownership: ownership, initialized: false,
                capabilities: capabilities, lastExit: lastExit, autostart: autostart,
                environments: environments)
        }
    }
#endif
