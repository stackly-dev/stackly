#if DEBUG
    import Foundation
    import StacklyClient

    /// The fixtures of Up (spec 0046; spec 0068 scenarios `environment-bringing-up` and
    /// `environment-up-incomplete`), built only from the generated contract types: the plan
    /// Bring Up sends, the `environment_up` operation of erp-development and its children, and
    /// the details while it runs and after it ended with services that are not ready.
    public nonisolated enum FixtureEnvironmentUp {
        /// The `environment_up` operation of erp-development.
        public static let upID = UUID(
            uuid: (
                0x01, 0x9a, 0x5f, 0x60, 0x7e, 0x10, 0x70, 0, 0x80, 0, 0, 0, 0, 0,
                FixtureID.operation, 0x60
            ))

        /// Child operation `number` of the Up.
        public static func childID(_ number: UInt8) -> UUID {
            UUID(
                uuid: (
                    0x01, 0x9a, 0x5f, 0x60 + number, 0x7e, 0x10, 0x70, 0, 0x80, 0, 0, 0, 0, 0,
                    FixtureID.operation, 0x60 + number
                ))
        }

        private static var erp: EnvironmentSummary {
            FixtureEnvironments.summaries.first { $0.id == FixtureEnvironments.erpID }
                ?? FixtureEnvironments.summaries[0]
        }

        // MARK: - Plan

        /// The plan of erp-development that Bring Up can send: keep postgres, start the
        /// borrowed queue, and create search with a secret that has no value yet.
        public static func upPlan() -> EnvPlan {
            let base = FixtureEnvironments.registeredPlan()
            let items = (base.items ?? []).map { item in
                guard item.action == .create else { return item }
                return PlanItem(
                    alias: item.alias, ownership: item.ownership, action: item.action,
                    steps: item.steps, instance: item.instance, provider: "official/clickhouse",
                    trust: .trusted, runtime: item.runtime,
                    endpoints: [PlanEndpoint(name: "client", host: "127.0.0.1", port: .auto)],
                    secrets: item.secrets, waitsFor: ["postgres"], differences: [],
                    checks: item.checks, providerRevision: nil, storage: nil)
            }
            return EnvPlan(
                validation: base.validation, registration: .registered,
                environment: base.environment, registeredLocation: base.registeredLocation,
                applicable: true, planDigest: String(repeating: "f", count: 64), items: items,
                order: base.order, blockers: [], detached: base.detached, needsSecrets: 1,
                lock: nil, local: nil)
        }

        // MARK: - Operations

        /// The `environment_up` operation of erp-development in `state`, with `done` of
        /// `total` services ended.
        public static func upOperation(
            state: OperationState = .running, done: UInt64 = 1, total: UInt64 = 4,
            failure: OperationFailureInfo? = nil, minutesAgo: Int = 1
        ) -> OperationSummary {
            let created = FixtureClock.minutesAgo(minutesAgo)
            let ended = state == .succeeded || state == .failed || state == .cancelled
            return OperationSummary(
                id: upID, kind: .environmentUp,
                resource: ResourceRef(
                    kind: .environment, id: FixtureEnvironments.erpID.uuidString.lowercased()),
                state: state, stage: ended ? nil : "services",
                progress: .counted(CountedProgress(done: done, total: total)), waitingOn: nil,
                cancelRequestedAt: nil, pointOfNoReturnAt: nil, failure: failure,
                recovery: nil, result: nil,
                trigger: OperationTriggerInfo(origin: .user, client: .gui), createdAt: created,
                startedAt: created,
                updatedAt: created.addingTimeInterval(ended ? 40 : 5), revision: 4,
                cancel: ended ? CancelAvailability.none : .available, parent: nil)
        }

        /// The failure of an Up that ended with `ready` of `total` services ready.
        public static func incompleteFailure(ready: Int, total: Int) -> OperationFailureInfo {
            OperationFailureInfo(
                reason: "step_failed", retryable: true,
                error: ErrorBody(
                    code: "env.up_incomplete", category: .failed,
                    message: "\(total - ready) of \(total) services are not ready",
                    retryable: true, resource: nil, fields: nil, hint: nil,
                    details: [
                        ErrorDetail(key: "ready", value: String(ready)),
                        ErrorDetail(key: "total", value: String(total)),
                    ], operationId: nil, rule: nil, items: nil))
        }

        /// A child of the Up: `kind` on the fixture instance `instance`.
        public static func child(
            _ number: UInt8, kind: OperationKind, instance: InstanceSummary,
            state: OperationState, failure: OperationFailureInfo? = nil, minutesAgo: Int = 1
        ) -> OperationSummary {
            let created = FixtureClock.minutesAgo(minutesAgo).addingTimeInterval(Double(number))
            let ended = state == .succeeded || state == .failed || state == .cancelled
            return OperationSummary(
                id: childID(number), kind: kind,
                resource: ResourceRef(kind: .instance, id: instance.id.uuidString.lowercased()),
                state: state, stage: ended ? nil : "readiness", progress: .indeterminate,
                waitingOn: nil, cancelRequestedAt: nil, pointOfNoReturnAt: nil,
                failure: failure, recovery: nil, result: nil,
                trigger: OperationTriggerInfo(origin: .user, client: .gui), createdAt: created,
                startedAt: created, updatedAt: created.addingTimeInterval(ended ? 20 : 3),
                revision: 3, cancel: ended ? CancelAvailability.none : .available, parent: upID)
        }

        /// The Up that ended with 2 of 4 ready and its three children, newest first: the
        /// coordinator, a failed start, and two starts that succeeded.
        public static var incompleteOperations: [OperationSummary] {
            let instances = FixtureEnvironments.instances
            return [
                upOperation(
                    state: .failed, done: 4, total: 4,
                    failure: incompleteFailure(ready: 2, total: 4), minutesAgo: 3),
                child(
                    3, kind: .startInstance, instance: instances[4], state: .failed,
                    failure: OperationFailureInfo(
                        reason: "step_failed", retryable: true,
                        error: ErrorBody(
                            code: "supervisor.readiness_timeout", category: .failed,
                            message: "readiness timed out", retryable: true, resource: nil,
                            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
                            items: nil)), minutesAgo: 3),
                child(
                    2, kind: .startInstance, instance: instances[2], state: .succeeded,
                    minutesAgo: 3),
                child(
                    1, kind: .startInstance, instance: instances[0], state: .succeeded,
                    minutesAgo: 3),
            ]
        }

        /// The reply of an accepted Up.
        public static func reply(_ operation: OperationSummary = upOperation(done: 0))
            -> EnvUpReply
        {
            EnvUpReply(
                environmentId: FixtureEnvironments.erpID, name: "erp-development",
                operation: OperationAccepted(operation: operation, replayed: false))
        }

        // MARK: - Details

        private static func notReady(
            _ reason: NotReadyReason, phase: UpPhase? = nil, waitingOn: [String] = [],
            references: [NeedsSecretItem] = [], dependency: String? = nil,
            dependencyReason: DependencyFailure? = nil, stage: UpStage? = nil,
            code: String? = nil, operationId: UUID? = nil, detail: String? = nil
        ) -> ServiceNotReady {
            ServiceNotReady(
                reason: reason, detail: detail, phase: phase, waitingOn: waitingOn,
                references: references, dependency: dependency,
                dependencyReason: dependencyReason, stage: stage, code: code,
                operationId: operationId)
        }

        private static func service(
            _ base: EnvironmentService, notReady: ServiceNotReady?
        ) -> EnvironmentService {
            EnvironmentService(
                alias: base.alias, provider: base.provider, ownership: base.ownership,
                selector: base.selector, instanceRef: base.instanceRef, instance: base.instance,
                notReady: notReady)
        }

        private static func state(
            _ alias: String, _ phase: UpPhase, stage: UpStage? = nil, code: String? = nil,
            dependency: String? = nil, dependencyReason: DependencyFailure? = nil,
            waitingOn: [String] = [], operationId: UUID? = nil,
            references: [NeedsSecretItem] = []
        ) -> ServiceUpState {
            ServiceUpState(
                alias: alias, phase: phase, stage: stage, code: code, dependency: dependency,
                dependencyReason: dependencyReason, waitingOn: waitingOn,
                operationId: operationId, references: references)
        }

        private static func summary(
            ready: UInt64, status: EnvironmentStatus, active: Bool
        ) -> EnvironmentSummary {
            EnvironmentSummary(
                id: erp.id, name: erp.name, revision: 3, manifestLocation: erp.manifestLocation,
                manifestSha256: erp.manifestSha256, lockSha256: nil, status: status,
                readyCount: ready, serviceCount: 4, updatedAt: FixtureClock.minutesAgo(1),
                activeOperation: active
                    ? EnvOperationRef(id: upID, kind: .environmentUp) : nil)
        }

        /// The summary of erp-development while its Up runs.
        public static var bringingUpSummary: EnvironmentSummary {
            summary(ready: 1, status: .partiallyReady, active: true)
        }

        /// The summaries of `env.list` while the Up of erp-development runs.
        public static var bringingUpSummaries: [EnvironmentSummary] {
            FixtureEnvironments.summaries.map {
                $0.id == FixtureEnvironments.erpID ? bringingUpSummary : $0
            }
        }

        /// erp-development while its Up runs: postgres is ready, cache starts, queue waits for
        /// postgres and cache, and search installs its runtime.
        public static func bringingUpDetail() -> EnvironmentDetail {
            let services = FixtureEnvironments.erpServices
            let rows = [
                service(services[0], notReady: nil),
                service(services[1], notReady: notReady(.bringingUp, phase: .starting)),
                service(
                    services[2],
                    notReady: notReady(
                        .bringingUp, phase: .waiting, waitingOn: ["postgres", "cache"])),
                service(services[3], notReady: notReady(.bringingUp, phase: .installing)),
            ]
            let report = EnvUpReport(
                operationId: upID, state: .running, startedAt: FixtureClock.minutesAgo(1),
                finishedAt: nil,
                services: [
                    state("postgres", .ready), state("cache", .starting),
                    state("queue", .waiting, waitingOn: ["postgres", "cache"]),
                    state("search", .installing),
                ])
            return EnvironmentDetail(
                summary: bringingUpSummary, services: rows, detached: [],
                manifestState: .inSync, problems: nil, lock: nil, local: nil, lastUp: report,
                lease: nil)
        }

        /// The reference the search service of the incomplete Up needs.
        public static let searchSecret = NeedsSecretItem(
            reference: "local:erp/search/adminPassword", reason: .missing, generateAllowed: true)

        /// A reference the daemon could not read because the Keychain is locked, and for which
        /// a generated value would not work: Set Secret… must pass both on as they came.
        public static let lockedSecret = NeedsSecretItem(
            reference: "local:erp/cache/password", reason: .storeLocked, generateAllowed: false)

        /// ``incompleteDetail()`` with the cache service stopped on ``lockedSecret`` and the
        /// search one on ``searchSecret``.
        public static func lockedSecretDetail() -> EnvironmentDetail {
            let incomplete = incompleteDetail()
            let services = (incomplete.services ?? []).map { row in
                row.alias == "cache"
                    ? service(row, notReady: notReady(.needsSecret, references: [lockedSecret]))
                    : row
            }
            return EnvironmentDetail(
                summary: incomplete.summary, services: services, detached: [],
                manifestState: .inSync, problems: nil, lock: nil, local: nil,
                lastUp: incomplete.lastUp, lease: nil)
        }

        /// erp-development after an Up that ended with 2 of 4 ready: queue failed to start and
        /// search needs a secret.
        public static func incompleteDetail() -> EnvironmentDetail {
            let services = FixtureEnvironments.erpServices
            let rows = [
                service(services[0], notReady: nil),
                service(services[1], notReady: nil),
                service(
                    services[2],
                    notReady: notReady(
                        .upFailed, stage: .start, code: "supervisor.readiness_timeout",
                        operationId: childID(3))),
                service(services[3], notReady: notReady(.needsSecret, references: [searchSecret])),
            ]
            let report = EnvUpReport(
                operationId: upID, state: .failed, startedAt: FixtureClock.minutesAgo(3),
                finishedAt: FixtureClock.minutesAgo(2),
                services: [
                    state("postgres", .ready), state("cache", .ready),
                    state(
                        "queue", .failed, stage: .start, code: "supervisor.readiness_timeout",
                        operationId: childID(3)),
                    state("search", .needsSecret, stage: .create, references: [searchSecret]),
                ])
            return EnvironmentDetail(
                summary: summary(ready: 2, status: .partiallyReady, active: false),
                services: rows, detached: [], manifestState: .inSync, problems: nil, lock: nil,
                local: nil, lastUp: report, lease: nil)
        }

        /// erp-development after its first Up ended with no service ready, so no instance
        /// exists yet and the Services table shows each one "Not created". `failed`: the create
        /// of postgres failed and the others waited for it, each with the reason of the Up
        /// report (rules 5 and 6 of *Alasan belum ready*). `interrupted`: the daemon stopped
        /// during the Up, whose report then holds no service, so each reason is the spec 0044
        /// one (rule 7).
        public static func notReadyDetail(_ upState: UpState = .failed) -> EnvironmentDetail {
            let services = FixtureEnvironments.erpServices.map { service in
                EnvironmentService(
                    alias: service.alias, provider: service.provider,
                    ownership: .dedicated, selector: service.selector ?? "stable",
                    instanceRef: nil, instance: nil, notReady: notReady(.notCreated))
            }
            let rows: [EnvironmentService]
            let report: [ServiceUpState]
            if upState == .interrupted {
                rows = services
                report = []
            } else {
                let skipped = { (base: EnvironmentService, reason: DependencyFailure) in
                    service(
                        base,
                        notReady: notReady(
                            .dependencyNotReady, dependency: "postgres",
                            dependencyReason: reason))
                }
                rows = [
                    service(
                        services[0],
                        notReady: notReady(
                            .upFailed, stage: .create, code: "supervisor.init_failed",
                            operationId: childID(1))),
                    skipped(services[1], .failed),
                    skipped(services[2], .failed),
                    skipped(services[3], .failed),
                ]
                report = [
                    state(
                        "postgres", .failed, stage: .create, code: "supervisor.init_failed",
                        operationId: childID(1)),
                    state("cache", .skipped, dependency: "postgres", dependencyReason: .failed),
                    state("queue", .skipped, dependency: "postgres", dependencyReason: .failed),
                    state("search", .skipped, dependency: "postgres", dependencyReason: .failed),
                ]
            }
            let upReport = EnvUpReport(
                operationId: upID, state: upState, startedAt: FixtureClock.minutesAgo(3),
                finishedAt: upState == .interrupted ? nil : FixtureClock.minutesAgo(2),
                services: report)
            return EnvironmentDetail(
                summary: summary(ready: 0, status: .down, active: false), services: rows,
                detached: [], manifestState: .inSync, problems: nil, lock: nil, local: nil,
                lastUp: upReport, lease: nil)
        }

        /// The replies of a scenario: `env.list` with `summaries`, and `env.get` with `detail`
        /// for its environment; the fixture link answers the others from
        /// ``FixtureEnvironments/detail(_:manifest:problems:files:)``.
        static func replies(_ summaries: [EnvironmentSummary], detail: EnvironmentDetail)
            -> [String: FixtureReply]
        {
            [
                "env.list": .reply(.envList(EnvironmentList(items: summaries))),
                "env.get": .reply(.envGet(detail)),
            ]
        }

        /// The summaries of `env.list` after the incomplete Up.
        public static var incompleteSummaries: [EnvironmentSummary] {
            FixtureEnvironments.summaries.map {
                $0.id == FixtureEnvironments.erpID ? incompleteDetail().summary : $0
            }
        }
    }
#endif
