#if DEBUG
    import Foundation
    import StacklyClient

    /// The Operations page fixtures (spec 0035, AC-19; spec 0068): one operation per state the
    /// page draws, with fixed titles and times, on the instances of
    /// ``FixtureScenario/instanceCases``, answered from data with no daemon.
    nonisolated enum FixtureOperations {
        /// A state of the page, named like its snapshots.
        enum Case: String, CaseIterable, Sendable {
            /// A start running with its stages, an install past the step that can’t be undone
            /// with Cancel off, a cancel that finishes safely, a failed start with Retry, a
            /// restore that needs recovery, a cancelled create, and a stop that is done.
            case list = "operations-list"
            /// The list with the restore that needs recovery open in the inspector, with
            /// its steps, created items, recovery, Resume, and Abandon.
            case inspector = "operations-inspector"
            /// No operation yet.
            case empty = "operations-empty"
            /// The first page of `operation.list` failed.
            case loadFailed = "operations-load-failed"
            /// A daemon without `operation.list`.
            case unsupported = "operations-unsupported"
            /// The list while stacklyd is unreachable: the last data, every mutation off.
            case offline = "operations-offline"
        }

        /// The operation of row `number` (1 is the oldest): a fixed ID like a UUID v7 whose
        /// first eight characters differ per row, so the short IDs of the rows differ too.
        static func id(_ number: UInt8) -> UUID {
            UUID(
                uuid: (
                    0x01, 0x9a, 0x5f, 0x30 + number, 0x7e, 0x10, 0x70, 0,
                    0x80, 0, 0, 0, 0, 0, FixtureID.operation, 0x30 + number
                ))
        }

        /// The instances of ``FixtureScenario/instanceCases`` the rows act on.
        private static var pg17: InstanceSummary { FixtureScenario.instanceCases[0] }
        private static var ordersDB: InstanceSummary { FixtureScenario.instanceCases[1] }
        private static var cache: InstanceSummary { FixtureScenario.instanceCases[2] }
        private static var mysql: InstanceSummary { FixtureScenario.instanceCases[3] }
        private static var analytics: InstanceSummary { FixtureScenario.instanceCases[5] }
        private static var legacy: InstanceSummary { FixtureScenario.instanceCases[6] }

        /// The restore that needs recovery, the one the inspector opens.
        static var recoveryID: UUID { id(3) }

        /// The rows of the list, newest first.
        static var rows: [OperationSummary] {
            [
                operation(
                    7, kind: .startInstance, resource: instance(pg17), state: .running,
                    stage: "readiness", cancel: .available, minutesAgo: 1),
                operation(
                    6, kind: .installRuntime,
                    resource: ResourceRef(
                        kind: .runtime, id: ordersDB.runtimeId.uuidString.lowercased()),
                    state: .running, stage: "commit",
                    progress: .counted(CountedProgress(done: 1, total: 2)),
                    cancel: .pastPointOfNoReturn, pointOfNoReturn: true, minutesAgo: 2),
                operation(
                    5, kind: .applyConfig, resource: instance(ordersDB),
                    state: .cancelRequested, stage: "apply", cancel: .continuing,
                    pointOfNoReturn: true, cancelRequested: true, minutesAgo: 4,
                    client: .cli),
                operation(
                    4, kind: .startInstance, resource: instance(analytics), state: .failed,
                    stage: "spawn", cancel: CancelAvailability.none,
                    failure: failure(
                        "supervisor.endpoint_in_use", "port 8123 is held by another process"),
                    minutesAgo: 9),
                operation(
                    3, kind: .restoreInstance, resource: instance(mysql), state: .needsRecovery,
                    stage: "swap", cancel: CancelAvailability.none,
                    recovery: RecoveryInfo(
                        hint: .resume, cause: .stepFailed,
                        error: ErrorBody(
                            code: "storage.io_error", category: .failed,
                            message: "the data folder could not be renamed", retryable: true,
                            resource: nil, fields: nil, hint: nil, details: nil,
                            operationId: nil, rule: nil, items: nil),
                        actions: [.resume, .abandon]),
                    pointOfNoReturn: true, minutesAgo: 30),
                operation(
                    2, kind: .createInstance, resource: instance(legacy), state: .cancelled,
                    stage: "init", cancel: CancelAvailability.none, cancelRequested: true,
                    minutesAgo: 75, client: .cli),
                operation(
                    1, kind: .stopInstance, resource: instance(cache), state: .succeeded,
                    cancel: CancelAvailability.none, minutesAgo: 120),
            ]
        }

        /// The operations of the list that have not ended, which the operation center holds.
        static var active: [OperationSummary] {
            rows.filter {
                switch $0.state {
                case .queued, .running, .cancelRequested, .needsRecovery: true
                case .succeeded, .failed, .cancelled: false
                }
            }
        }

        /// The detail `operation.get` answers for an operation of the list.
        static func detail(of operation: OperationSummary) -> OperationDetail {
            guard operation.id == recoveryID else {
                return OperationDetail(operation: operation, steps: [], created: [])
            }
            let at = operation.startedAt ?? operation.createdAt
            return OperationDetail(
                operation: operation,
                steps: [
                    step(1, "validate", .done, at: at),
                    step(2, "stage", .done, at: at.addingTimeInterval(4)),
                    step(
                        3, "swap", .failed, at: at.addingTimeInterval(41), attempts: 2,
                        errorCode: "storage.io_error"),
                ],
                created: [
                    CreatedItemInfo(
                        kind: "folder",
                        reference: "instances/mysql-erp/restore-staging",
                        state: .created, detail: nil),
                    CreatedItemInfo(
                        kind: "folder", reference: "instances/mysql-erp/data.previous",
                        state: .kept, detail: "the data before the restore"),
                ])
        }

        /// The service of a case: `operation.list` answers the rows of the case in one page,
        /// `operation.get` their detail, and every mutation is not sent.
        static func service(_ fixture: Case) -> OperationsService {
            OperationsService(
                list: { _ throws(ClientError) in
                    switch fixture {
                    case .loadFailed:
                        throw .rejected(
                            ErrorBody(
                                code: "internal.unexpected", category: .internal,
                                message: "the registry could not be read", retryable: false,
                                resource: nil, fields: nil, hint: nil, details: nil,
                                operationId: nil, rule: nil, items: nil))
                    case .empty, .unsupported:
                        return OperationPage(items: [], next: nil)
                    case .list, .inspector, .offline:
                        return OperationPage(items: rows, next: nil)
                    }
                },
                get: { id throws(ClientError) in
                    guard let found = rows.first(where: { $0.id == id }) else {
                        throw .rejected(.fixtureNotFound(.operation, id: id.uuidString))
                    }
                    return detail(of: found)
                },
                send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) })
        }

        /// The operation center of a case: the operations that have not ended, or
        /// `request.unknown_method` for an older daemon. Load its store before use.
        @MainActor
        static func center(_ fixture: Case) -> OperationCenter {
            OperationCenter(
                fetchPage: { _ throws(ClientError) in
                    switch fixture {
                    case .unsupported:
                        throw .rejected(
                            ErrorBody(
                                code: "request.unknown_method", category: .unsupported,
                                message: "unknown method", retryable: false, resource: nil,
                                fields: nil, hint: nil, details: nil, operationId: nil,
                                rule: nil, items: nil))
                    case .empty, .loadFailed:
                        return Page(items: [], next: nil)
                    case .list, .inspector, .offline:
                        return Page(items: active, next: nil)
                    }
                },
                fetchOne: { _ throws(ClientError) in nil },
                now: { FixtureClock.now }, sleep: { _ in })
        }

        // MARK: - Builders

        private static func instance(_ summary: InstanceSummary) -> ResourceRef {
            ResourceRef(kind: .instance, id: summary.id.uuidString.lowercased())
        }

        private static func operation(
            _ number: UInt8, kind: OperationKind, resource: ResourceRef?,
            state: OperationState, stage: String? = nil,
            progress: OperationProgress = .indeterminate, cancel: CancelAvailability?,
            failure: OperationFailureInfo? = nil, recovery: RecoveryInfo? = nil,
            pointOfNoReturn: Bool = false, cancelRequested: Bool = false, minutesAgo: Int,
            client: ClientKind = .gui
        ) -> OperationSummary {
            let created = FixtureClock.minutesAgo(minutesAgo)
            let ended = state == .succeeded || state == .failed || state == .cancelled
            return OperationSummary(
                id: id(number), kind: kind, resource: resource, state: state, stage: stage,
                progress: progress, waitingOn: nil,
                cancelRequestedAt: cancelRequested ? created.addingTimeInterval(30) : nil,
                pointOfNoReturnAt: pointOfNoReturn ? created.addingTimeInterval(20) : nil,
                failure: failure, recovery: recovery, result: nil,
                trigger: OperationTriggerInfo(origin: .user, client: client),
                createdAt: created, startedAt: created,
                updatedAt: ended ? created.addingTimeInterval(45) : created.addingTimeInterval(5),
                revision: 3, cancel: cancel)
        }

        private static func failure(_ code: String, _ message: String) -> OperationFailureInfo {
            OperationFailureInfo(
                reason: "step_failed", retryable: true,
                error: ErrorBody(
                    code: code, category: .conflict, message: message, retryable: true,
                    resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
                    rule: nil, items: nil))
        }

        private static func step(
            _ seq: UInt32, _ name: String, _ state: StepState, at: Date, attempts: UInt32 = 1,
            errorCode: String? = nil
        ) -> OperationStepInfo {
            OperationStepInfo(
                seq: seq, name: name, state: state, repeatable: true, reversible: true,
                attempts: attempts, startedAt: at,
                finishedAt: state == .running ? nil : at.addingTimeInterval(3),
                errorCode: errorCode)
        }
    }
#endif
