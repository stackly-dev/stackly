import Foundation
import Testing

@testable import StacklyUI

/// The copy confirmation and the copy button (spec 0067, AC-4).
struct CopyConfirmationTests {
    /// What the injected writer and announcer saw.
    private final class Recorder {
        var writes: [String] = []
        var announcements: [String] = []
    }

    private func make(
        _ recorder: Recorder,
        _ sleeps: ManualSleeps
    ) -> CopyConfirmation {
        CopyConfirmation(
            write: { recorder.writes.append($0) },
            announce: { recorder.announcements.append($0) },
            sleep: { try await sleeps.sleep($0) }
        )
    }

    @Test func aCopyWritesOnceAnnouncesOnceAndConfirmsForTwoSeconds() async {
        let recorder = Recorder()
        let sleeps = ManualSleeps()
        let confirmation = make(recorder, sleeps)
        #expect(confirmation.isConfirming == false)

        let task = Task {
            await confirmation.copy("127.0.0.1:5432", announcement: "Address copied.")
        }
        await sleeps.waitUntilWaiting(1)
        #expect(recorder.writes == ["127.0.0.1:5432"])
        #expect(recorder.announcements == ["Address copied."])
        #expect(confirmation.isConfirming)
        #expect(await sleeps.durations == [.seconds(2)])

        await sleeps.releaseAll()
        await task.value
        #expect(confirmation.isConfirming == false)
        #expect(recorder.writes.count == 1)
        #expect(recorder.announcements.count == 1)
    }

    @Test func aSecondPressWritesAgainAndStartsTheTwoSecondsOver() async {
        let recorder = Recorder()
        let sleeps = ManualSleeps()
        let confirmation = make(recorder, sleeps)

        let first = Task { await confirmation.copy("a", announcement: "A copied.") }
        await sleeps.waitUntilWaiting(1)
        let second = Task { await confirmation.copy("b", announcement: "B copied.") }
        await sleeps.waitUntilWaiting(2)
        #expect(recorder.writes == ["a", "b"])
        #expect(recorder.announcements == ["A copied.", "B copied."])
        #expect(await sleeps.durations == [.seconds(2), .seconds(2)])

        await sleeps.releaseFirst()
        await first.value
        #expect(confirmation.isConfirming, "the first wait ended, but the second still holds")

        await sleeps.releaseFirst()
        await second.value
        #expect(confirmation.isConfirming == false)
    }

    @Test func aCancelledWaitLeavesTheNextCopyInCharge() async {
        let recorder = Recorder()
        let sleeps = ManualSleeps()
        let confirmation = make(recorder, sleeps)

        let first = Task { await confirmation.copy("a", announcement: "A copied.") }
        await sleeps.waitUntilWaiting(1)
        first.cancel()
        let second = Task { await confirmation.copy("b", announcement: "B copied.") }
        await sleeps.waitUntilWaiting(2)
        await sleeps.releaseFirst()
        await first.value
        #expect(confirmation.isConfirming)
        await sleeps.releaseAll()
        await second.value
        #expect(confirmation.isConfirming == false)
    }

    @Test func theDefaultAnnouncementCapitalizesWhatWasCopied() {
        #expect(CopyButton.defaultAnnouncement(for: "address") == "Address copied.")
        #expect(CopyButton.defaultAnnouncement(for: "instance ID") == "Instance ID copied.")
        #expect(CopyButton.defaultAnnouncement(for: "") == " copied.")
    }
}
