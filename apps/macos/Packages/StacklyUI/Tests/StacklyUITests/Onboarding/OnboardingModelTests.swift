import Foundation
import Testing

@testable import StacklyUI

/// The record, the model, and the footer label of the onboarding (spec 0016, AC-1 to AC-3).
struct OnboardingModelTests {
    /// Every value the footer can get for the agent: none, and each of the six states.
    nonisolated private static let agentStates: [BackgroundAgentState?] =
        [nil] + BackgroundAgentState.allCases.map(Optional.some)

    // MARK: - Record and launch (AC-1)

    @Test func theOnboardingShowsAtStepOneWhenTheRecordIsEmpty() {
        let model = OnboardingModel(store: InMemoryOnboardingRecordStore())
        #expect(model.isPresented)
        #expect(model.step == .welcome)
    }

    @Test func theOnboardingStaysHiddenWhenTheRecordSaysFinished() {
        let model = OnboardingModel(store: InMemoryOnboardingRecordStore(isFinished: true))
        #expect(model.isPresented == false)
    }

    @Test func theUserDefaultsStoreReadsAndWritesTheOnboardingFinishedKey() throws {
        let suite = "onboarding-test-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = UserDefaultsOnboardingRecordStore(defaults: defaults)
        #expect(store.isFinished == false)
        #expect(defaults.object(forKey: "onboarding.finished") == nil)
        store.markFinished()
        #expect(store.isFinished)
        #expect(defaults.bool(forKey: "onboarding.finished"))
        #expect(UserDefaultsOnboardingRecordStore.key == "onboarding.finished")
    }

    @Test func aRecordThatSaysNoShowsTheOnboardingAgain() throws {
        let suite = "onboarding-test-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set(false, forKey: "onboarding.finished")
        let model = OnboardingModel(store: UserDefaultsOnboardingRecordStore(defaults: defaults))
        #expect(model.isPresented)
    }

    @Test func movingBetweenStepsWritesNothing() {
        let store = InMemoryOnboardingRecordStore()
        let model = OnboardingModel(store: store)
        model.next()
        model.next()
        model.back()
        model.go(to: .commandLine)
        model.go(to: .welcome)
        #expect(store.writes == 0)
        #expect(store.isFinished == false)
        #expect(model.isPresented)
    }

    @Test func theStepSurvivesAWindowThatClosesAndOpensBecauseTheModelIsNotTheWindows() {
        // The runtime owns the model, so a window that opens again reads the same instance.
        let model = OnboardingModel(store: InMemoryOnboardingRecordStore())
        model.go(to: .storage)
        #expect(model.isPresented)
        #expect(model.step == .storage)
    }

    // MARK: - Steps and footer (AC-2)

    @Test func thereAreExactlyFiveStepsWithTheirLabels() {
        #expect(
            OnboardingStep.allCases.map(\.label)
                == ["Welcome", "Background Agent", "Storage", "Catalog Trust", "Command Line"])
        #expect(OnboardingStep.allCases.map(\.number) == [1, 2, 3, 4, 5])
    }

    @Test func nextAndBackStopAtTheEnds() {
        let model = OnboardingModel(store: InMemoryOnboardingRecordStore())
        model.back()
        #expect(model.step == .welcome)
        for _ in 0..<10 { model.next() }
        #expect(model.step == .commandLine)
        model.back()
        #expect(model.step == .catalogTrust)
    }

    @Test func aStepDotMovesStraightToItsStep() {
        let model = OnboardingModel(store: InMemoryOnboardingRecordStore())
        model.go(to: .catalogTrust)
        #expect(model.step == .catalogTrust)
        model.go(to: .backgroundAgent)
        #expect(model.step == .backgroundAgent)
    }

    @Test func theFooterTextNamesTheStepAndTheCount() {
        #expect(OnboardingFooter.stepText(.welcome) == "Step 1 of 5 · Welcome")
        #expect(OnboardingFooter.stepText(.backgroundAgent) == "Step 2 of 5 · Background Agent")

    }

    @Test func backShowsFromTheSecondStepOn() {
        #expect(OnboardingFooter.showsBack(step: .welcome) == false)
        for step in OnboardingStep.allCases.dropFirst() {
            #expect(OnboardingFooter.showsBack(step: step))
        }
    }

    @Test(arguments: agentStates)
    func theLastStepSaysGetStartedWhateverTheAgentSays(state: BackgroundAgentState?) {
        #expect(
            OnboardingFooter.primaryLabel(step: .commandLine, agentState: state) == "Get Started")
    }

    @Test(arguments: agentStates)
    func theAgentStepSaysNotNowUntilTheAgentIsAllowed(state: BackgroundAgentState?) {
        let label = OnboardingFooter.primaryLabel(step: .backgroundAgent, agentState: state)
        #expect(label == (state == .enabled ? "Continue" : "Not Now"))
    }

    @Test(arguments: agentStates)
    func otherStepsSayContinueWhateverTheAgentSays(state: BackgroundAgentState?) {
        for step in [OnboardingStep.welcome, .storage, .catalogTrust] {
            #expect(OnboardingFooter.primaryLabel(step: step, agentState: state) == "Continue")
        }
    }

    // MARK: - Finish and present (AC-3)

    @Test(arguments: [OnboardingFinish.getStarted, .skipped])
    func bothWaysOfFinishingWriteTheSameRecordAndHideTheOnboarding(reason: OnboardingFinish) {
        let store = InMemoryOnboardingRecordStore()
        let model = OnboardingModel(store: store)
        model.go(to: .storage)
        model.finish(reason)
        #expect(store.isFinished)
        #expect(store.writes == 1)
        #expect(model.isPresented == false)
    }

    @Test func finishingWhileHiddenDoesNothing() {
        let store = InMemoryOnboardingRecordStore(isFinished: true)
        let model = OnboardingModel(store: store)
        model.finish(.skipped)
        #expect(store.writes == 0)
    }

    @Test func presentShowsStepOneAgainWithoutChangingTheRecord() {
        let store = InMemoryOnboardingRecordStore()
        let model = OnboardingModel(store: store)
        model.go(to: .catalogTrust)
        model.finish(.skipped)
        model.present()
        #expect(model.isPresented)
        #expect(model.step == .welcome)
        #expect(store.isFinished)
        #expect(store.writes == 1)
    }

    @Test func presentWhileShowingKeepsTheStep() {
        let model = OnboardingModel(store: InMemoryOnboardingRecordStore())
        model.go(to: .storage)
        model.present()
        #expect(model.step == .storage)
    }
}
