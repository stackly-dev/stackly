import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The copy confirmation and the copy button (spec 0067, AC-4).
struct CopyConfirmationTests {
    /// What the injected writer saw.
    private final class Recorder {
        var writes: [String] = []
    }

    private func make(
        _ recorder: Recorder,
        _ sleeps: ManualSleeps
    ) -> CopyConfirmation {
        CopyConfirmation(
            write: { recorder.writes.append($0) },

            sleep: { try await sleeps.sleep($0) }
        )
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "Spec 0067 AC-4: a second press writes again and restarts the two seconds"))
    func aSecondPressWritesAgainAndStartsTheTwoSecondsOver() async {
        let recorder = Recorder()
        let sleeps = ManualSleeps()
        let confirmation = make(recorder, sleeps)

        let first = Task { await confirmation.copy("a") }
        await sleeps.waitUntilWaiting(1)
        let second = Task { await confirmation.copy("b") }
        await sleeps.waitUntilWaiting(2)
        #expect(recorder.writes == ["a", "b"])

        #expect(await sleeps.durations == [.seconds(2), .seconds(2)])

        await sleeps.releaseFirst()
        await first.value
        #expect(confirmation.isConfirming, "the first wait ended, but the second still holds")

        await sleeps.releaseFirst()
        await second.value
        #expect(confirmation.isConfirming == false)
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected:
                "Spec 0067 AC-4: a cancelled wait never ends the confirmation of the next copy"))
    func aCancelledWaitLeavesTheNextCopyInCharge() async {
        let recorder = Recorder()
        let sleeps = ManualSleeps()
        let confirmation = make(recorder, sleeps)

        let first = Task { await confirmation.copy("a") }
        await sleeps.waitUntilWaiting(1)
        first.cancel()
        let second = Task { await confirmation.copy("b") }
        await sleeps.waitUntilWaiting(2)
        await sleeps.releaseFirst()
        await first.value
        #expect(confirmation.isConfirming)
        await sleeps.releaseAll()
        await second.value
        #expect(confirmation.isConfirming == false)
    }

}
