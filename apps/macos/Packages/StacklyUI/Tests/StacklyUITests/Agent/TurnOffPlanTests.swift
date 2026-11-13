import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The words of the Turn Off confirmation (spec 0031, AC-19, AC-20).
@Suite(.timeLimit(.minutes(1)))
struct TurnOffPlanTests {
    private func preview(
        running: Int, stopping: Int = 0, skipped: [(String, String)] = []
    ) -> StopAllPreview {
        StopAllPreview(
            items: (0..<running).map { number in
                StopAllItem(
                    instanceId: UUID(), name: "pg\(number)", lifecycle: .running, demand: [],
                    autostart: InstanceAutostart(policy: .manual, session: .none), environments: nil
                )
            },
            alreadyStopping: (0..<stopping).map { _ in InstanceRef(instanceId: UUID(), name: "x") },
            skipped: skipped.map {
                StopAllSkip(instanceId: UUID(), name: $0.0, code: $0.1, environments: nil)
            },
            atLoginCount: 0, affectedEnvironments: nil)
    }

    @Test func theBodyCountsRunningAndStoppingInstancesAndTheOnesSetToStartAtLogin() {
        let plan = TurnOffPlan(preview: preview(running: 2, stopping: 1), atLoginCount: 4)
        #expect(plan.title == "Turn Off Background Agent?")
        #expect(
            plan.message
                == "3 running instances will stop. 4 instances set to start at login won’t start until you turn it on again."
        )
        #expect(plan.canTurnOff)
    }

    @Test func oneInstanceUsesTheSingularForm() {
        let plan = TurnOffPlan(preview: preview(running: 1), atLoginCount: 1)
        #expect(
            plan.message
                == "1 running instance will stop. 1 instance set to start at login won’t start until you turn it on again."
        )
    }

    @Test func aSkippedInstanceNamesItsReasonAndDisablesTurnOff() {
        let plan = TurnOffPlan(
            preview: preview(running: 0, skipped: [("held", "supervisor.stop_requires_override")]),
            atLoginCount: 0)
        #expect(
            plan.message.hasSuffix(
                "Turn Off can’t finish while these instances run: held · an environment uses it."))
        #expect(plan.canTurnOff == false)
    }

    @Test func aRefusalListsTheInstancesThatBlockedTheTurnOff() {
        let stopped = UUID()
        let held = UUID()
        let text = TurnOffPlan.refusal(
            result: StopAllResult(
                stopped: [stopped], failed: [],
                skipped: [StopAllEntry(instanceId: held, code: "supervisor.stop_requires_override")]
            ),
            names: [held: "held"])
        #expect(
            text
                == "Background agent is still on. Stopped 1 instances. These couldn’t be stopped: held · an environment uses it."
        )
    }
}
