import Testing

@testable import StacklyUI

/// Get Started, Continue, and Escape (spec 0016, AC-2, AC-3, AC-11).
struct OnboardingActionsTests {
    /// What the feedback center announced.
    final class Spoken {
        var texts: [String] = []
    }

    private static func center(_ spoken: Spoken) -> FeedbackCenter {
        FeedbackCenter(announce: { spoken.texts.append($0) })
    }

    @Test func getStartedFinishesAndShowsSetupCompleteOnce() {
        let store = InMemoryOnboardingRecordStore()
        let onboarding = OnboardingModel(store: store)
        let spoken = Spoken()
        let center = Self.center(spoken)
        onboarding.go(to: .commandLine)
        OnboardingActions.primary(onboarding: onboarding, feedback: center)
        #expect(store.isFinished)
        #expect(onboarding.isPresented == false)
        #expect(center.current?.message == "Setup complete.")
        #expect(center.current?.tone == .positive)
        #expect(spoken.texts == ["Setup complete."])
    }

    @Test func continueOnlyMovesToTheNextStep() {
        let store = InMemoryOnboardingRecordStore()
        let onboarding = OnboardingModel(store: store)
        let spoken = Spoken()
        let center = Self.center(spoken)
        for expected in OnboardingStep.allCases.dropFirst() {
            OnboardingActions.primary(onboarding: onboarding, feedback: center)
            #expect(onboarding.step == expected)
        }
        #expect(store.writes == 0)
        #expect(center.current == nil)
        #expect(spoken.texts.isEmpty)
    }

    @Test(arguments: OnboardingStep.allCases)
    func escapeOnAnyStepWritesTheSameRecordAndShowsNothing(step: OnboardingStep) {
        let store = InMemoryOnboardingRecordStore()
        let onboarding = OnboardingModel(store: store)
        onboarding.go(to: step)
        OnboardingActions.skip(onboarding: onboarding)
        #expect(store.isFinished)
        #expect(store.writes == 1)
        #expect(onboarding.isPresented == false)
    }

    @Test func getStartedWithoutAFeedbackCenterStillFinishes() {
        let store = InMemoryOnboardingRecordStore()
        let onboarding = OnboardingModel(store: store)
        onboarding.go(to: .commandLine)
        OnboardingActions.primary(onboarding: onboarding, feedback: nil)
        #expect(store.isFinished)
    }
}
