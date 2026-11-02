#if DEBUG
    import Foundation
    import StacklyClient

    /// The restart plans and previews of the Restart Daemon fixtures and tests (spec 0030),
    /// built from the generated DTOs with the fixed IDs and the fixed clock of spec 0068.
    public nonisolated enum FixtureRestart {
        /// The ID of the plan every fixture shows.
        public static let planID = FixtureID.uuid(kind: FixtureID.daemon, number: 30)

        /// When the plan was written: five minutes before the fixed "now".
        public static let createdAt = FixtureClock.minutesAgo(5)

        /// One instance of a plan.
        /// - Parameters:
        ///   - number: The instance number, for the ID.
        ///   - name: The instance name.
        ///   - decision: What the restart does with it.
        ///   - reason: Why.
        ///   - stopped: Whether the old daemon stopped it already.
        ///   - outcome: What became of it, or `nil` while the plan is open.
        ///   - failureCode: The failure code of a failed outcome.
        public static func instance(
            _ number: UInt8, _ name: String, decision: PlanDecision = .keep,
            reason: PlanReason = .handoverProven, stopped: Bool = false,
            outcome: PlanOutcome? = nil, failureCode: String? = nil
        ) -> RestartPlanInstance {
            RestartPlanInstance(
                instanceId: FixtureID.uuid(kind: FixtureID.instance, number: number),
                name: name, decision: decision, reason: reason,
                stoppedAt: stopped ? createdAt.addingTimeInterval(2) : nil,
                readyAt: outcome == .restored || outcome == .adopted
                    ? createdAt.addingTimeInterval(9) : nil,
                outcome: outcome, failureCode: failureCode)
        }

        /// A plan.
        /// - Parameters:
        ///   - state: Where it is.
        ///   - instances: Its instances.
        ///   - closeReason: Why an aborted or expired plan closed.
        public static func plan(
            _ state: PlanState, instances: [RestartPlanInstance],
            closeReason: PlanCloseReason? = nil
        ) -> RestartPlanView {
            let handedOver = createdAt.addingTimeInterval(3)
            let reconciled = createdAt.addingTimeInterval(5)
            let finished = createdAt.addingTimeInterval(9)
            return RestartPlanView(
                id: planID, state: state, createdAt: createdAt,
                handedOverAt: [.handedOver, .restoring, .finished].contains(state)
                    ? handedOver : nil,
                reconciledAt: [.restoring, .finished].contains(state) ? reconciled : nil,
                finishedAt: [.finished, .aborted, .expired].contains(state) ? finished : nil,
                closeReason: closeReason, instances: instances)
        }

        /// The usual mix: two instances that keep running and one that stops.
        public static var mixedInstances: [RestartPlanInstance] {
            [
                instance(1, "pg17-dev"),
                instance(2, "nats-local", reason: .handoverProven),
                instance(
                    3, "valkey-cache", decision: .stopRestore, reason: .handoverUnproven,
                    stopped: true),
            ]
        }

        /// A preview of a restart that stops one instance and waits for one operation.
        public static func preview(
            digest: String = "d1g3st", launchContext: LaunchContextKind = .launchd,
            blocking: [BlockingOperation] = []
        ) -> RestartPreview {
            RestartPreview(
                digest: digest, launchContext: launchContext, supervisorAvailable: true,
                instances: [
                    RestartPreviewInstance(
                        instanceId: FixtureID.uuid(kind: FixtureID.instance, number: 1),
                        name: "pg17-dev", provider: "official/postgresql", lifecycle: .running,
                        decision: .keep, reason: .handoverProven),
                    RestartPreviewInstance(
                        instanceId: FixtureID.uuid(kind: FixtureID.instance, number: 2),
                        name: "nats-local", provider: "official/nats", lifecycle: .running,
                        decision: .keep, reason: .handoverProven),
                    RestartPreviewInstance(
                        instanceId: FixtureID.uuid(kind: FixtureID.instance, number: 3),
                        name: "valkey-cache", provider: "official/valkey", lifecycle: .running,
                        decision: .stopRestore, reason: .handoverUnproven),
                ],
                blockingOperations: blocking)
        }
    }
#endif
