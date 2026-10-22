import Testing

@testable import StacklyUI

/// Where keyboard focus goes after a card action (spec 0016, *Kartu Background Agent*).
struct CardFocusRuleTests {
    private static let missing = ShellStatus.unreachable(
        lastKnownAt: nil, since: BackgroundAgentStepContentTests.since)

    private static func card(
        _ state: BackgroundAgentState, isWorking: Bool = false, isStarting: Bool = false
    ) -> BackgroundAgentStepContent {
        BackgroundAgentStepContentTests.make(
            BackgroundAgentStepContentTests.input(
                state, status: missing, code: "ipc.endpoint_missing", isStarting: isStarting),
            isWorking: isWorking)
    }

    @Test func aFailedTurnOnKeepsTheRowAndReturnsFocusToTheAction() {
        let outcome = CardFocusRule.outcome(
            old: Self.card(.notRegistered, isWorking: true), new: Self.card(.notRegistered),
            focused: nil, afterAction: true)
        #expect(outcome == .init(target: .cardAction, releasesFlag: true))
    }

    @Test func aTurnOnThatEndsInApprovalFocusesTheNewAction() {
        let outcome = CardFocusRule.outcome(
            old: Self.card(.notRegistered, isWorking: true), new: Self.card(.requiresApproval),
            focused: nil, afterAction: true)
        #expect(outcome == .init(target: .cardAction, releasesFlag: true))
    }

    @Test func aTurnOnThatStartsTheDaemonMovesFocusToThePrimaryButton() {
        let outcome = CardFocusRule.outcome(
            old: Self.card(.notRegistered, isWorking: true),
            new: Self.card(.enabled, isWorking: true, isStarting: true),
            focused: nil, afterAction: true)
        #expect(outcome == .init(target: .primary, releasesFlag: true))
    }

    @Test func theEndOfTheWaitAfterwardsDoesNotPullFocusBack() {
        let outcome = CardFocusRule.outcome(
            old: Self.card(.enabled, isWorking: true, isStarting: true),
            new: Self.card(.enabled), focused: .primary, afterAction: false)
        #expect(outcome == .init(target: nil, releasesFlag: false))
    }

    @Test func nothingChangingWhileAnActionWorksLeavesFocusAlone() {
        let working = Self.card(.notRegistered, isWorking: true)
        let outcome = CardFocusRule.outcome(
            old: working, new: working, focused: nil, afterAction: true)
        #expect(outcome == .init(target: nil, releasesFlag: false))
    }

    @Test func withoutAPressFocusOnTheActionStaysWhenTheNewRowHasOne() {
        let outcome = CardFocusRule.outcome(
            old: Self.card(.requiresApproval), new: Self.card(.disabled), focused: .cardAction,
            afterAction: false)
        #expect(outcome == .init(target: .cardAction, releasesFlag: false))
    }

    @Test func withoutAPressFocusOnTheActionMovesToPrimaryWhenTheNewRowHasNone() {
        let outcome = CardFocusRule.outcome(
            old: Self.card(.requiresApproval), new: Self.card(.notFound), focused: .cardAction,
            afterAction: false)
        #expect(outcome == .init(target: .primary, releasesFlag: false))
    }

    @Test func focusElsewhereIsNeverPulledToTheCard() {
        let outcome = CardFocusRule.outcome(
            old: Self.card(.requiresApproval), new: Self.card(.disabled), focused: .primary,
            afterAction: false)
        #expect(outcome == .init(target: nil, releasesFlag: false))
    }

    @Test func onlyAMoveToAnotherTargetReleasesTheFlag() {
        #expect(CardFocusRule.releasesFlag(whenFocusMovesTo: .primary))
        #expect(CardFocusRule.releasesFlag(whenFocusMovesTo: .cardAction) == false)
        #expect(CardFocusRule.releasesFlag(whenFocusMovesTo: nil) == false)
    }
}
