import StacklyUI
import XCTest

/// Pointer smoke tests using the labels of native controls.
@MainActor
final class SmokeUITests: XCTestCase {
    private func launch(_ scenario: FixtureScenario.ID) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["STACKLY_UI_SCENARIO"] = scenario.rawValue
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launch()
        XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 10))
        return app
    }

    func testOnboardingContinuesThroughAllFiveSteps() {
        let app = launch(.onboarding)
        let titles = [
            "Step 1 of 5 · Welcome", "Step 2 of 5 · Background Agent", "Step 3 of 5 · Storage",
            "Step 4 of 5 · Catalog Trust", "Step 5 of 5 · Command Line",
        ]
        let buttons = ["Continue", "Not Now", "Continue", "Continue", "Get Started"]
        for (title, button) in zip(titles, buttons) {
            XCTAssertTrue(app.staticTexts[title].firstMatch.waitForExistence(timeout: 5))
            app.buttons[button].firstMatch.click()
        }
        XCTAssertTrue(app.staticTexts["Setup complete."].firstMatch.waitForExistence(timeout: 5))
    }

    func testShellShowsItsMainWindow() {
        let app = launch(.connectedMixed)
        XCTAssertTrue(app.windows.firstMatch.exists)
        XCTAssertTrue(app.staticTexts["Overview"].firstMatch.waitForExistence(timeout: 5))
    }
}
