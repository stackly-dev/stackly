#if DEBUG
    import Foundation
    import StacklyClient

    /// The fixtures of Down (spec 0047; spec 0068 scenario `environment-bringing-down`), built
    /// only from the generated contract types: the preview of erp-development, its
    /// `environment_down` operation and the stops under it, and the detail while it runs.
    public nonisolated enum FixtureEnvironmentDown {
        /// The `environment_down` operation of erp-development.
        public static let downID = UUID(
            uuid: (
                0x01, 0x9a, 0x5f, 0x70, 0x7e, 0x10, 0x70, 0, 0x80, 0, 0, 0, 0, 0,
                FixtureID.operation, 0x70
            ))

        /// Child stop `number` of the Down.
        public static func childID(_ number: UInt8) -> UUID {
            UUID(
                uuid: (
                    0x01, 0x9a, 0x5f, 0x70 + number, 0x7e, 0x10, 0x70, 0, 0x80, 0, 0, 0, 0, 0,
                    FixtureID.operation, 0x70 + number
                ))
        }

        private static var erp: EnvironmentSummary {
            FixtureEnvironments.summaries.first { $0.id == FixtureEnvironments.erpID }
                ?? FixtureEnvironments.summaries[0]
        }

        private static func instance(_ number: Int) -> InstanceSummary {
            FixtureEnvironments.instances[number - 1]
        }

        private static func member(
            _ number: Int, alias: String?, ownership: EnvOwnership?
        ) -> DownInstance {
            let summary = instance(number)
            return DownInstance(
                instanceId: summary.id, name: summary.name, alias: alias, ownership: ownership,
                lifecycle: summary.lifecycle)
        }

        /// The environment reporting, as a holder of a lease on `pg17-dev`.
        public static let reportingHolder = InstanceEnvironment(
            environmentId: FixtureEnvironments.reportingID, name: "reporting", alias: "db",
            ownership: .shared, leaseActive: true)

        // MARK: - Preview

        /// The Down preview of erp-development: it stops `cache`, keeps `pg17-dev` for
        /// reporting and `queue` that you started, and holds three leases. `cancelsUp` adds
        /// an Up in progress; `blocked` adds an instance Stackly can't confirm.
        public static func preview(cancelsUp: Bool = false, blocked: Bool = false)
            -> EnvDownPreview
        {
            EnvDownPreview(
                environment: EnvironmentRef(id: erp.id, name: erp.name, revision: erp.revision),
                stop: [member(3, alias: "cache", ownership: .dedicated)],
                keep: [
                    DownKeep(
                        instance: member(1, alias: "postgres", ownership: .dedicated),
                        demand: [.environment], environments: [reportingHolder]),
                    DownKeep(
                        instance: member(5, alias: "queue", ownership: .shared),
                        demand: [.manual], environments: []),
                ],
                idle: [],
                blocked: blocked
                    ? [
                        DownBlocked(
                            instance: member(7, alias: "legacy", ownership: .dedicated),
                            code: "supervisor.identity_unproven")
                    ] : [],
                leaseCount: 3, cancelsUp: cancelsUp)
        }

        /// A preview with nothing to do: no instance to stop, no active lease, and no Up.
        public static func emptyPreview() -> EnvDownPreview {
            EnvDownPreview(
                environment: EnvironmentRef(id: erp.id, name: erp.name, revision: erp.revision),
                stop: [], keep: [], idle: [member(3, alias: "cache", ownership: .dedicated)],
                blocked: [], leaseCount: 0, cancelsUp: false)
        }

        // MARK: - Operations

        /// The `environment_down` operation of erp-development in `state`, with `done` of
        /// `total` stops ended, at `stage`.
        public static func downOperation(
            state: OperationState = .running, stage: String? = "stop", done: UInt64 = 1,
            total: UInt64 = 2, result: OperationResult? = nil,
            failure: OperationFailureInfo? = nil, minutesAgo: Int = 1
        ) -> OperationSummary {
            let created = FixtureClock.minutesAgo(minutesAgo)
            let ended = state == .succeeded || state == .failed || state == .cancelled
            return OperationSummary(
                id: downID, kind: .environmentDown,
                resource: ResourceRef(
                    kind: .environment, id: FixtureEnvironments.erpID.uuidString.lowercased()),
                state: state, stage: stage,
                progress: .counted(CountedProgress(done: done, total: total)), waitingOn: nil,
                cancelRequestedAt: nil, pointOfNoReturnAt: nil, failure: failure,
                recovery: nil, result: result,
                trigger: OperationTriggerInfo(origin: .user, client: .gui), createdAt: created,
                startedAt: created,
                updatedAt: created.addingTimeInterval(ended ? 20 : 4), revision: 4,
                cancel: ended ? CancelAvailability.none : .available, parent: nil)
        }

        /// The result of a Down that stopped `stopped` instances and kept `inUse` because a
        /// demand came after the preview.
        public static func result(stopped: Int, inUse: Int = 0) -> OperationResult {
            OperationResult(
                resources: (0..<stopped).map { index in
                    ResourceRef(
                        kind: .instance, id: instance(3 + index).id.uuidString.lowercased())
                },
                partial: (0..<inUse).map { index in
                    StacklyClient.PartialItem(
                        item: instance(1 + index).name,
                        resource: ResourceRef(
                            kind: .instance, id: instance(1 + index).id.uuidString.lowercased()),
                        outcome: .skipped, code: "env.instance_in_use")
                })
        }

        /// The failure of a Down that stopped `stopped` of `total` instances: each of
        /// `instances` ended with `outcome`.
        public static func incompleteFailure(
            stopped: Int, total: Int, instances: [String], outcome: String = "failed"
        ) -> OperationFailureInfo {
            OperationFailureInfo(
                reason: "step_failed", retryable: false,
                error: ErrorBody(
                    code: "env.down_incomplete", category: .failed,
                    message: "\(total - stopped) of \(total) instances did not stop",
                    retryable: false, resource: nil, fields: nil,
                    hint:
                        "bring the environment down again, or stop the instances listed; nothing was deleted",
                    details: [
                        ErrorDetail(key: "stopped", value: String(stopped)),
                        ErrorDetail(key: "total", value: String(total)),
                        ErrorDetail(key: "count", value: String(instances.count)),
                    ], operationId: nil, rule: nil,
                    items: instances.map { name in
                        ErrorItem(
                            kind: "instance",
                            fields: [
                                ErrorDetail(key: "name", value: name),
                                ErrorDetail(key: "outcome", value: outcome),
                                ErrorDetail(
                                    key: "code",
                                    value: outcome == "left_running"
                                        ? "env.down_changed" : "supervisor.stop_timeout"),
                            ])
                    }))
        }

        /// A stop under the Down: `instance` in `state`.
        public static func child(
            _ number: UInt8, instance: InstanceSummary, state: OperationState,
            failure: OperationFailureInfo? = nil, minutesAgo: Int = 3
        ) -> OperationSummary {
            let created = FixtureClock.minutesAgo(minutesAgo).addingTimeInterval(Double(number))
            return OperationSummary(
                id: childID(number), kind: .stopInstance,
                resource: ResourceRef(kind: .instance, id: instance.id.uuidString.lowercased()),
                state: state, stage: nil, progress: .indeterminate, waitingOn: nil,
                cancelRequestedAt: nil, pointOfNoReturnAt: nil, failure: failure,
                recovery: nil, result: nil,
                trigger: OperationTriggerInfo(origin: .user, client: .gui), createdAt: created,
                startedAt: created, updatedAt: created.addingTimeInterval(15), revision: 3,
                cancel: CancelAvailability.none, parent: downID)
        }

        /// The Down that stopped 1 of 2 instances and its two stops, newest first: the
        /// coordinator, a stop that timed out, and one that succeeded.
        public static var incompleteOperations: [OperationSummary] {
            [
                downOperation(
                    state: .failed, stage: "stop", done: 2, total: 2,
                    failure: incompleteFailure(stopped: 1, total: 2, instances: ["queue"]),
                    minutesAgo: 3),
                child(
                    2, instance: instance(5), state: .failed,
                    failure: OperationFailureInfo(
                        reason: "step_failed", retryable: true,
                        error: ErrorBody(
                            code: "supervisor.stop_timeout", category: .failed,
                            message: "the process did not stop in time", retryable: true,
                            resource: nil, fields: nil, hint: nil, details: nil,
                            operationId: nil, rule: nil, items: nil))),
                child(1, instance: instance(3), state: .succeeded),
            ]
        }

        /// The reply of an accepted Down.
        public static func reply(_ operation: OperationSummary = downOperation(done: 0))
            -> EnvDownReply
        {
            EnvDownReply(
                environmentId: FixtureEnvironments.erpID, name: "erp-development",
                operation: OperationAccepted(operation: operation, replayed: false))
        }

        // MARK: - Details

        /// The summary of erp-development while its Down runs.
        public static var bringingDownSummary: EnvironmentSummary {
            EnvironmentSummary(
                id: erp.id, name: erp.name, revision: 4, manifestLocation: erp.manifestLocation,
                manifestSha256: erp.manifestSha256, lockSha256: nil, status: .partiallyReady,
                readyCount: 1, serviceCount: 4, updatedAt: FixtureClock.minutesAgo(1),
                activeOperation: EnvOperationRef(id: downID, kind: .environmentDown))
        }

        /// The summaries of `env.list` while the Down of erp-development runs.
        public static var bringingDownSummaries: [EnvironmentSummary] {
            FixtureEnvironments.summaries.map {
                $0.id == FixtureEnvironments.erpID ? bringingDownSummary : $0
            }
        }

        /// erp-development while its Down runs: the fixture services, with the lease already
        /// released.
        public static func bringingDownDetail() -> EnvironmentDetail {
            let base =
                FixtureEnvironments.detail(FixtureEnvironments.erpID)
                ?? FixtureEnvironmentUp.incompleteDetail()
            return EnvironmentDetail(
                summary: bringingDownSummary, services: base.services, detached: base.detached,
                manifestState: .inSync, problems: nil, lock: base.lock, local: base.local,
                lastUp: nil, lease: EnvLeaseInfo(activeCount: 0, heldSince: nil))
        }

        /// The replies of the scenario: `env.list` and `env.get` while the Down runs, and the
        /// preview.
        static var replies: [String: FixtureReply] {
            var replies = FixtureEnvironmentUp.replies(
                bringingDownSummaries, detail: bringingDownDetail())
            replies["env.preview_down"] = .reply(.envPreviewDown(preview()))
            return replies
        }
    }
#endif
