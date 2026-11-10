#if DEBUG
    import Foundation
    import StacklyClient

    /// The Activity tab fixtures (spec 0035, AC-19; spec 0068): the tab of `pg17-dev` in each
    /// of its states, answered from data with no daemon, with one row per action of *Teks
    /// Activity*.
    nonisolated enum FixtureActivity {
        /// A state of the tab, named like its snapshots.
        enum Case: String, CaseIterable, Sendable {
            /// One row per action: operations of every template, a failure, a cancel, the
            /// recovery actions, the secret rows, and an action this build does not know.
            case rows = "activity-rows"
            /// No row yet.
            case empty = "activity-empty"
            /// The first page of `audit.list` failed.
            case failed = "activity-failed"
            /// A daemon without `audit.list`.
            case unsupported = "activity-unsupported"
        }

        /// The instance of every row: `pg17-dev`.
        static let instance = FixtureID.uuid(kind: FixtureID.instance, number: 1)

        /// The ID of row `number`.
        static func id(_ number: UInt8) -> UUID {
            FixtureID.uuid(kind: 0x35, number: number)
        }

        /// The operation of row `number`.
        static func operation(_ number: UInt8) -> UUID {
            FixtureID.uuid(kind: FixtureID.operation, number: 0x60 + number)
        }

        private static var reference: String {
            "local:instance/\(instance.uuidString.lowercased())/superuser-password"
        }

        /// The rows of ``Case/rows``, newest first.
        static var rows: [AuditEventInfo] {
            [
                row(
                    20, kind: .changeEndpoints, client: .cli, minutesAgo: 4, revision: 4,
                    details: ["oldRevision": "3", "ports": "database 5432 → 5433"]),
                row(
                    19, action: "secret.reveal", minutesAgo: 12,
                    details: ["purpose": "copy", "reference": reference]),
                row(
                    18, action: "secret.reveal", minutesAgo: 13,
                    details: ["purpose": "show", "reference": reference]),
                row(17, kind: .applyConfig, minutesAgo: 40, revision: 3),
                row(
                    16, kind: .startInstance, result: .failed, minutesAgo: 62, revision: 2,
                    details: [
                        "errorCode": "supervisor.endpoint_in_use", "reason": "step_failed",
                        "stage": "spawn",
                    ]),
                row(
                    15, kind: .stopInstance, actor: .daemon, client: nil, minutesAgo: 70,
                    details: ["state": "succeeded"]),
                row(14, kind: .restartInstance, result: .cancelled, client: .cli, minutesAgo: 95),
                row(
                    13, action: "operation.cancel", kind: .restartInstance, client: .cli,
                    minutesAgo: 96, details: ["disposition": "requested"]),
                row(
                    12, kind: .restoreInstance, result: .failed, minutesAgo: 180,
                    details: ["errorCode": "operation.abandoned", "reason": "abandoned"]),
                row(
                    11, action: "operation.abandon", kind: .restoreInstance, minutesAgo: 181,
                    details: ["leftovers": "2"]),
                row(10, action: "operation.resume", kind: .restoreInstance, minutesAgo: 230),
                row(
                    9, action: "operation.needs_recovery", kind: .restoreInstance,
                    result: .needsRecovery, minutesAgo: 240,
                    details: ["cause": "step_failed", "hint": "resume"]),
                row(8, kind: .reloadInstance, minutesAgo: 60 * 26, revision: 2),
                row(7, kind: .forceStopInstance, client: .cli, minutesAgo: 60 * 30),
                row(
                    6, kind: .renameInstance, minutesAgo: 60 * 50,
                    details: ["newName": "pg17-dev", "oldName": "pg17"]),
                row(5, kind: .resolveConfigDrift, minutesAgo: 60 * 52, revision: 2),
                row(
                    4, action: "secret.set", client: .cli, minutesAgo: 60 * 72,
                    details: ["reference": reference]),
                row(
                    21, action: "secret.delete", minutesAgo: 60 * 80,
                    details: ["reference": reference]),
                row(
                    3, action: "backup.verified", actor: .user, client: .unrecognized,
                    minutesAgo: 60 * 96),
                row(2, kind: .startInstance, minutesAgo: 60 * 24 * 40, revision: 1),
                row(
                    1, kind: .createInstance, client: .cli, minutesAgo: 60 * 24 * 400,
                    revision: 1,
                    details: [
                        "duplicateOf": FixtureID.uuid(kind: FixtureID.instance, number: 9)
                            .uuidString.lowercased(),
                        "duplicateOfName": "pg16-main", "provider": "PostgreSQL",
                    ]),
            ]
        }

        /// The read of a case: `audit.list` answers the rows of the case in one page.
        static func read(_ fixture: Case)
            -> @Sendable (UUID, String?) async throws(ClientError)
            -> AuditPage
        {
            { _, _ throws(ClientError) in
                switch fixture {
                case .rows:
                    return AuditPage(items: rows, next: nil)
                case .empty:
                    return AuditPage(items: [], next: nil)
                case .failed:
                    throw .rejected(
                        ErrorBody(
                            code: "internal.unexpected", category: .internal,
                            message: "the registry could not be read", retryable: false,
                            resource: nil, fields: nil, hint: nil, details: nil,
                            operationId: nil, rule: nil, items: nil))
                case .unsupported:
                    throw .rejected(
                        ErrorBody(
                            code: "request.unknown_method", category: .unsupported,
                            message: "unknown method", retryable: false, resource: nil,
                            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
                            items: nil))
                }
            }
        }

        /// The model of a case, with its first page loaded.
        @MainActor
        static func model(_ fixture: Case) async -> InstanceActivityModel {
            let model = InstanceActivityModel(
                instanceId: instance, read: read(fixture), now: { FixtureClock.now })
            await model.reload()
            return model
        }

        // MARK: - Builders

        private static func row(
            _ number: UInt8, action: String = "operation.finished",
            kind: OperationKind? = nil, result: AuditResultKind = .succeeded,
            actor: AuditActorKind = .user, client: ClientKind? = .gui, minutesAgo: Int,
            revision: UInt64? = nil, details: [String: String] = [:]
        ) -> AuditEventInfo {
            let isOperation = action.hasPrefix("operation.")
            return AuditEventInfo(
                id: id(number), occurredAt: FixtureClock.minutesAgo(minutesAgo), actor: actor,
                client: client, action: action, operationKind: isOperation ? kind : nil,
                operationId: isOperation ? operation(number) : nil,
                resource: ResourceRef(kind: .instance, id: instance.uuidString.lowercased()),
                instanceId: instance, configRevision: revision, result: result,
                details: details.sorted { $0.key < $1.key }.map {
                    AuditDetail(key: $0.key, value: $0.value)
                })
        }
    }
#endif
