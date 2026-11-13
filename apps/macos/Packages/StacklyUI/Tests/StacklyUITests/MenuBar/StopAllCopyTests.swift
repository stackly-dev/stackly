import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The *Copy Stop All* table of spec 0032, row by row.
struct StopAllCopyTests {
    private func item(_ name: String, _ number: UInt8) -> StopAllItem {
        StopAllItem(
            instanceId: UUID(uuid: (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, number)),
            name: name, lifecycle: .running, demand: [.manual],
            autostart: InstanceAutostart(policy: .manual, session: .none), environments: nil)
    }

    private func preview(_ items: [StopAllItem], atLogin: UInt64 = 0) -> StopAllPreview {
        StopAllPreview(
            items: items, alreadyStopping: [], skipped: [], atLoginCount: atLogin,
            affectedEnvironments: nil)
    }

    @Test func demandSourcesAreJoinedInOrder() {
        #expect(StopAllCopy.demand([.manual]) == "Started by you")
        #expect(
            StopAllCopy.demand([.manual, .startup, .environment])
                == "Started by you, Started at login, Used by an environment")
        #expect(StopAllCopy.demand([]) == "")
    }

    @Test func rowsFooterAndReasonsUseTheTableWords() {
        #expect(StopAllCopy.title == "Stop All Services?")
        #expect(StopAllCopy.alreadyStopping("cache") == "cache · already stopping")
        #expect(
            StopAllCopy.skipped("a", code: "supervisor.identity_unproven")
                == "a · won’t stop: Stackly can’t confirm this process")
        #expect(
            StopAllCopy.skipped("a", code: "supervisor.stop_requires_override")
                == "a · won’t stop: used by an environment")
        #expect(
            StopAllCopy.skipped("a", code: "operation.recovery_required")
                == "a · won’t stop: an operation for it needs your attention in Stackly")
        #expect(
            StopAllCopy.footer(preview([], atLogin: 0))
                == "Data is kept. None of them start at your next login.")
        #expect(
            StopAllCopy.footer(preview([], atLogin: 1))
                == "Data is kept. 1 of them starts again at your next login.")
        #expect(
            StopAllCopy.footer(preview([], atLogin: 3))
                == "Data is kept. 3 of them start again at your next login.")
        #expect(StopAllCopy.nothingRunning == "Nothing is running.")
        #expect(StopAllCopy.cancel == "Cancel" && StopAllCopy.confirm == "Stop All")
    }

    @Test func aFinishedStopSaysHowManyStopped() {
        let shown = preview([item("a", 1), item("b", 2), item("c", 3)])
        let all = StopAllResult(
            stopped: shown.items.map(\.instanceId), failed: [], skipped: [])
        let outcome = StopAllCopy.outcome(of: all, preview: shown)
        #expect(outcome.text == "Stopped 3 services. Data is kept.")
        #expect(outcome.opensStackly == false)
    }

    @Test func oneFailureNamesItAndOffersOpenStackly() {
        let shown = preview([item("a", 1), item("b", 2)])
        let result = StopAllResult(
            stopped: [shown.items[0].instanceId],
            failed: [StopAllEntry(instanceId: shown.items[1].instanceId, code: "x")],
            skipped: [])
        let outcome = StopAllCopy.outcome(of: result, preview: shown)
        #expect(outcome.text == "Stopped 1 of 2. b didn’t stop.")
        #expect(outcome.opensStackly)
    }

    @Test func manyFailuresCountAndListThemAndSkippedAreCounted() {
        let shown = preview([item("a", 1), item("b", 2), item("c", 3)])
        let result = StopAllResult(
            stopped: [shown.items[0].instanceId],
            failed: [
                StopAllEntry(instanceId: shown.items[1].instanceId, code: "x"),
                StopAllEntry(instanceId: shown.items[2].instanceId, code: "x"),
            ],
            skipped: [StopAllEntry(instanceId: UUID(), code: "supervisor.identity_unproven")])
        let outcome = StopAllCopy.outcome(of: result, preview: shown)
        #expect(outcome.text == "Stopped 1 of 3. 2 didn’t stop: b, c. 1 skipped.")
    }

    @Test func aCancelledStopSaysSoAndOffersOpenStackly() {
        let shown = preview([item("a", 1), item("b", 2)])
        let result = StopAllResult(
            stopped: [shown.items[0].instanceId], failed: [],
            skipped: [
                StopAllEntry(instanceId: shown.items[1].instanceId, code: "operation.cancelled")
            ])
        let outcome = StopAllCopy.outcome(of: result, preview: shown)
        #expect(outcome.text == "Stop All was cancelled. Stopped 1 of 2.")
        #expect(outcome.opensStackly)
    }
}
