import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The feedback center and the hold rule of the card (spec 0067, AC-9, AC-11).
struct FeedbackCenterTests {

    private func make(
        _ sleeps: ManualSleeps
    ) -> FeedbackCenter {
        FeedbackCenter(

            sleep: { try await sleeps.sleep($0) }
        )
    }

    private let good = Feedback.success("Instance started.")
    private let bad = Feedback(tone: .critical, message: "Failed.", detail: "Try again.")

    // MARK: - show

    @Test func persistentFeedbackReplacesPersistentFeedback() {
        let center = make(ManualSleeps())
        let other = Feedback(tone: .caution, message: "Careful.")
        center.show(bad)
        center.show(other)
        #expect(center.current?.id == other.id)
    }

    @Test func dismissEmptiesTheCenter() {
        let center = make(ManualSleeps())
        center.show(bad)
        center.dismiss()
        #expect(center.current == nil)
    }

    // MARK: - countdown

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-9: the countdown clears transient feedback after its visible duration"
        ))
    func countdownClearsTransientFeedbackAfterItsVisibleDuration() async {
        let sleeps = ManualSleeps()
        let center = make(sleeps)
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
        let center = make(sleeps)
        center.show(good)
        let task = Task { await center.countdown(good.id) }
        await sleeps.waitUntilWaiting(1)
        center.show(bad)
        await sleeps.releaseAll()
        await task.value
        #expect(center.current?.id == bad.id)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"], expected: "Spec 0067 AC-9: a cancelled countdown clears nothing"))
    func aCancelledCountdownClearsNothing() async {
        let sleeps = ManualSleeps()
        let center = make(sleeps)
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
        let center = make(sleeps)
        center.show(bad)
        await center.countdown(bad.id)
        center.dismiss()
        center.show(good)
        await center.countdown(UUID())
        #expect(await sleeps.durations.isEmpty, "nothing slept")
        #expect(center.current?.id == good.id)
    }

    // MARK: - FeedbackHold

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-9: the countdown runs only when no hover, focus, or inactive window holds the card"
        ))
    func theCountdownRunsOnlyWhenNothingHoldsTheCard() {
        for hover in [false, true] {
            do {
                for active in [false, true] {
                    let hold = FeedbackHold(
                        isHovered: hover, isWindowActive: active)
                    #expect(hold.countsDown == (!hover && active))
                }
            }
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-024"], expected: "Spec 0067 AC-9: the wait starts over when a hold ends"))
    func theCardStartsItsWaitFromTheStartWhenAHoldEnds() throws {
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

        #expect(code.contains("HitTarget.minimum"))
        #expect(code.contains("toneSurface("))
        #expect(code.contains("Material") == false)
    }
}
