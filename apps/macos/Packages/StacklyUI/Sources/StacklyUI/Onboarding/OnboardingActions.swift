import Foundation

/// What the primary button and Escape do (spec 0016, AC-2, AC-3, AC-11), apart from the
/// view so a test runs them without a window.
enum OnboardingActions {
    /// The primary button. Continue and Not Now only move to the next step. Get Started on
    /// the last step finishes the onboarding and shows "Setup complete." in the window's
    /// feedback card, which `FeedbackCenter` also announces once.
    ///
    /// - Parameters:
    ///   - onboarding: The onboarding model.
    ///   - feedback: The window's feedback center, or `nil` where there is none.
    @MainActor static func primary(onboarding: OnboardingModel, feedback: FeedbackCenter?) {
        guard onboarding.step == .commandLine else {
            onboarding.next()
            return
        }
        onboarding.finish(.getStarted)
        feedback?.show(.success(OnboardingCopy.setupComplete))
    }

    /// Escape, and the VoiceOver escape action, on any step: the same record as Get Started,
    /// and no feedback.
    ///
    /// - Parameter onboarding: The onboarding model.
    @MainActor static func skip(onboarding: OnboardingModel) {
        onboarding.finish(.skipped)
    }
}
