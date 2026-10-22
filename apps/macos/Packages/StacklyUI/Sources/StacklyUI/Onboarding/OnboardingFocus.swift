import Foundation

/// Where keyboard focus can sit that the onboarding moves it to (spec 0016, *Kartu
/// Background Agent*, focus after a card action).
nonisolated enum OnboardingFocusTarget: Hashable {
    /// The button on the right of the Background Agent card.
    case cardAction
    /// The primary button of the footer.
    case primary
}

/// Where focus goes when the card changes row. The button that was pressed usually goes
/// away or turns into another, and a disabled button drops focus while it works, so the rule
/// does not rest on where focus is when the row changes.
nonisolated enum CardFocusRule {
    /// What to do with focus after the card changed.
    struct Outcome: Equatable {
        /// The target to focus, or `nil` to leave focus as it is.
        var target: OnboardingFocusTarget?
        /// Whether the pending flag of a pressed card action is released.
        var releasesFlag: Bool
    }

    /// Decides where focus goes.
    ///
    /// - Parameters:
    ///   - old: The card before the change.
    ///   - new: The card after.
    ///   - focused: Where focus is now.
    ///   - afterAction: Whether a card action was pressed and focus is still owed.
    static func outcome(
        old: BackgroundAgentStepContent, new: BackgroundAgentStepContent,
        focused: OnboardingFocusTarget?, afterAction: Bool
    ) -> Outcome {
        let rowChanged = BackgroundAgentStepContent.isSameRow(old, new) == false
        let finishedWorking = old.isWorking && new.isWorking == false
        let hasActiveAction = new.action != nil && new.isWorking == false
        if afterAction {
            guard rowChanged || finishedWorking else {
                return Outcome(target: nil, releasesFlag: false)
            }
            return Outcome(target: hasActiveAction ? .cardAction : .primary, releasesFlag: true)
        }
        guard focused == .cardAction, rowChanged else {
            return Outcome(target: nil, releasesFlag: false)
        }
        return Outcome(target: new.action != nil ? .cardAction : .primary, releasesFlag: false)
    }

    /// Whether moving focus to `target` by the user releases the flag: only a move to a
    /// target other than the card action does, and losing focus does not.
    static func releasesFlag(whenFocusMovesTo target: OnboardingFocusTarget?) -> Bool {
        target != nil && target != .cardAction
    }
}
