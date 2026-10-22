import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the onboarding on the `onboarding` scenario (spec 0016, step 7, and spec
/// 0068, AC-14): the five steps with the primary button, Escape on step 3, and Get Started.
///
/// The labels the tests expect are written out here, not read from `OnboardingCopy`, so a
/// change of copy shows up as a failed test.
@MainActor
final class OnboardingUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    /// The footer text of each step, in order.
    static let steps = [
        "Step 1 of 5 · Welcome", "Step 2 of 5 · Background Agent", "Step 3 of 5 · Storage",
        "Step 4 of 5 · Catalog Trust", "Step 5 of 5 · Command Line",
    ]

    /// Launches the app on the `onboarding` scenario and waits for the primary button.
    private func launchOnboarding(appearance: Appearance = .light) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["STACKLY_UI_SCENARIO"] = FixtureScenario.ID.onboarding.rawValue
        app.launchEnvironment["STACKLY_UI_APPEARANCE"] = appearance.rawValue
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launch()
        let primary = app.element(AccessibilityID.Onboarding.primary)
        XCTAssertTrue(primary.waitForExistence(timeout: 10), "the onboarding did not appear")
        return app
    }

    func testTheOnboardingFillsTheWindowWithoutASidebar() throws {
        try evidence(
            covers: ["AC-1"], expected: "a first launch shows the onboarding and no sidebar")
        let app = launchOnboarding()
        XCTAssertTrue(app.staticTexts["Welcome to Stackly"].exists)
        XCTAssertFalse(app.element(AccessibilityID.Sidebar.row(.overview)).exists)
        XCTAssertFalse(app.element(AccessibilityID.DaemonFooter.status).exists)
        XCTAssertFalse(app.element(AccessibilityID.Onboarding.back).exists, "no Back on step 1")
    }

    func testThePrimaryButtonWalksTheFiveStepsAndGetStartedShowsSetupComplete() throws {
        try evidence(
            covers: ["AC-2", "AC-11"],
            expected: "the primary button walks five steps and Get Started shows the feedback card"
        )
        let app = launchOnboarding()
        let primary = app.element(AccessibilityID.Onboarding.primary)
        let labels = ["Continue", "Not Now", "Continue", "Continue", "Get Started"]
        for (index, label) in labels.enumerated() {
            XCTAssertTrue(
                app.staticTexts[Self.steps[index]].waitForExistence(timeout: 5),
                "step \(index + 1) text")
            XCTAssertEqual(primary.label, label, "primary button on step \(index + 1)")
            XCTAssertEqual(
                app.element(AccessibilityID.Onboarding.back).exists, index > 0,
                "Back from step 2")
            if index < labels.count - 1 { primary.click() }
        }
        primary.click()
        let overview = app.element(AccessibilityID.Sidebar.row(.overview))
        XCTAssertTrue(overview.waitForExistence(timeout: 5), "the shell shows after Get Started")
        let card = app.element(AccessibilityID.Feedback.card)
        XCTAssertTrue(card.waitForExistence(timeout: 5))
        XCTAssertTrue(card.label.contains("Setup complete."), "the card says \(card.label)")
    }

    func testEscapeOnStepThreeShowsTheShellWithoutFeedback() throws {
        try evidence(
            covers: ["AC-3", "AC-11"],
            expected: "Escape on step 3 shows the shell at Overview and no feedback card")
        let app = launchOnboarding()
        app.element(AccessibilityID.Onboarding.step(3)).click()
        XCTAssertTrue(app.staticTexts[Self.steps[2]].waitForExistence(timeout: 5))
        app.typeKey(.escape, modifierFlags: [])
        let overview = app.element(AccessibilityID.Sidebar.row(.overview))
        XCTAssertTrue(overview.waitForExistence(timeout: 5), "the shell shows after Escape")
        XCTAssertFalse(app.element(AccessibilityID.Feedback.card).exists)
        XCTAssertFalse(app.element(AccessibilityID.Onboarding.primary).exists)
    }

    func testAStepDotMovesStraightToItsStep() throws {
        try evidence(
            covers: ["AC-2", "AC-10"], expected: "a step dot moves straight to its step")
        let app = launchOnboarding()
        app.element(AccessibilityID.Onboarding.step(4)).click()
        XCTAssertTrue(app.staticTexts[Self.steps[3]].waitForExistence(timeout: 5))
        app.element(AccessibilityID.Onboarding.step(1)).click()
        XCTAssertTrue(app.staticTexts[Self.steps[0]].waitForExistence(timeout: 5))
    }
}
