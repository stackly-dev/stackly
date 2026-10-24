import Foundation

/// What the onboarding footer says (spec 0016, AC-2). Pure, so the labels are tested
/// without a view.
nonisolated public enum OnboardingFooter {
    /// The label of the primary button.
    /// - Parameters:
    ///   - step: The step on screen.
    ///   - agentState: The background agent's state, or `nil` while it is not read or the
    ///     build has no agent.
    /// - Returns: `Get Started` on the last step, `Not Now` on the Background Agent step
    ///   until the agent is allowed, otherwise `Continue`.
    public static func primaryLabel(step: OnboardingStep, agentState: BackgroundAgentState?)
        -> String
    {
        switch step {
        case .commandLine:
            OnboardingCopy.getStarted
        case .backgroundAgent where agentState != .enabled:
            OnboardingCopy.notNow
        default:
            OnboardingCopy.continue
        }
    }

    /// The text next to the step dots, such as `Step 2 of 5 · Background Agent`.
    public static func stepText(_ step: OnboardingStep) -> String {
        OnboardingCopy.stepText(number: step.number, label: step.label)
    }

    /// Whether the Back button shows: from the second step on.
    public static func showsBack(step: OnboardingStep) -> Bool {
        step != .welcome
    }
}

extension OnboardingFooter {

}
