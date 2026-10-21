import Foundation
import Testing

@testable import StacklyUI

/// The feedback center and the hold rule of the card (spec 0067, AC-9, AC-11).
struct FeedbackCenterTests {
    private final class Announcements {
        var texts: [String] = []
    }

    private func make(
        _ announcements: Announcements,
        _ sleeps: ManualSleeps
    ) -> FeedbackCenter {
        FeedbackCenter(
            announce: { announcements.texts.append($0) },
            sleep: { try await sleeps.sleep($0) }
        )
    }

    private let good = Feedback.success("Instance started.")
    private let bad = Feedback(tone: .critical, message: "Failed.", detail: "Try again.")

    // MARK: - show

    @Test func showReplacesWhatIsOnScreenAndAnnouncesTheMessageThenTheDetailOnce() {
        let announcements = Announcements()
        let center = make(announcements, ManualSleeps())
        #expect(center.current == nil)
        center.show(good)
        #expect(center.current?.id == good.id)
        center.show(bad)
        #expect(center.current?.id == bad.id)
        #expect(announcements.texts == ["Instance started.", "Failed. Try again."])
    }

    @Test func goodNewsOnlyAnnouncesWhilePersistentFeedbackIsOnScreen() {
        let announcements = Announcements()
        let center = make(announcements, ManualSleeps())
        center.show(bad)
        center.show(good)
        #expect(center.current?.id == bad.id, "the failure stays")
        #expect(announcements.texts == ["Failed. Try again.", "Instance started."])
    }

    @Test func persistentFeedbackReplacesPersistentFeedback() {
        let center = make(Announcements(), ManualSleeps())
        let other = Feedback(tone: .caution, message: "Careful.")
        center.show(bad)
        center.show(other)
        #expect(center.current?.id == other.id)
    }

    @Test func dismissEmptiesTheCenter() {
        let center = make(Announcements(), ManualSleeps())
        center.show(bad)
        center.dismiss()
        #expect(center.current == nil)
    }

    // MARK: - countdown

    @Test func countdownClearsTransientFeedbackAfterItsVisibleDuration() async {
        let sleeps = ManualSleeps()
        let center = make(Announcements(), sleeps)
        center.show(good)
        let task = Task { await center.countdown(good.id) }
        await sleeps.waitUntilWaiting(1)
        let durations = await sleeps.durations
        #expect(durations == [good.visibleDuration])
        #expect(center.current != nil)
        await sleeps.releaseAll()
        await task.value
        #expect(center.current == nil)
    }

    @Test func countdownLeavesFeedbackThatReplacedItAlone() async {
        let sleeps = ManualSleeps()
        let center = make(Announcements(), sleeps)
        center.show(good)
        let task = Task { await center.countdown(good.id) }
        await sleeps.waitUntilWaiting(1)
        center.show(bad)
        await sleeps.releaseAll()
        await task.value
        #expect(center.current?.id == bad.id)
    }

    @Test func aCancelledCountdownClearsNothing() async {
        let sleeps = ManualSleeps()
        let center = make(Announcements(), sleeps)
        center.show(good)
        let task = Task { await center.countdown(good.id) }
        await sleeps.waitUntilWaiting(1)
        task.cancel()
        await sleeps.cancelAll()
        await task.value
        #expect(center.current?.id == good.id)
    }

    @Test func countdownReturnsAtOnceForPersistentFeedbackAndForAnOldID() async {
        let sleeps = ManualSleeps()
        let center = make(Announcements(), sleeps)
        center.show(bad)
        await center.countdown(bad.id)
        center.dismiss()
        center.show(good)
        await center.countdown(UUID())
        #expect(await sleeps.durations.isEmpty, "nothing slept")
        #expect(center.current?.id == good.id)
    }

    // MARK: - FeedbackHold

    @Test func theCountdownRunsOnlyWhenNothingHoldsTheCard() {
        for hover in [false, true] {
            for focus in [false, true] {
                for active in [false, true] {
                    let hold = FeedbackHold(
                        isHovered: hover, isFocused: focus, isWindowActive: active)
                    #expect(hold.countsDown == (!hover && !focus && active))
                }
            }
        }
    }

    @Test func theCardStartsItsWaitFromTheStartWhenAHoldEnds() throws {
        let path =
            "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/Feedback/FeedbackCard.swift"
        let code = SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
        )
        .codeLines.joined(separator: "\n")
        #expect(
            code.contains(".task(id: CountdownKey(id: feedback.id, countsDown: hold.countsDown))"))
        #expect(code.contains("await center.countdown(feedback.id)"))
        #expect(code.contains(".safeAreaInset(edge: .bottom, spacing: 0)"))
        #expect(code.contains("StacklyMotion.crossfade"))
        #expect(code.contains(#".accessibilityLabel("Dismiss")"#))
        #expect(code.contains("HitTarget.minimum"))
        #expect(code.contains("toneSurface("))
        #expect(code.contains("Material") == false)
    }
}
