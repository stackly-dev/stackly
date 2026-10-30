import StacklyUI
import XCTest

/// How a UI test starts the Debug app on a fixture scenario (spec 0068).
enum Appearance: String {
    case light
    case dark
}

extension XCTestCase {
    /// Launches the app on `scenario` and waits for a text that shows the window is ready:
    /// the Overview row of the sidebar by default (the onboarding has no sidebar).
    func launch(
        _ scenario: FixtureScenario.ID, appearance: Appearance = .light,
        readyText: String = "Overview"
    ) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["STACKLY_UI_SCENARIO"] = scenario.rawValue
        app.launchEnvironment["STACKLY_UI_APPEARANCE"] = appearance.rawValue
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launch()
        let ready = app.staticTexts[readyText].firstMatch
        XCTAssertTrue(ready.waitForExistence(timeout: 10), "the window did not appear")
        return app
    }
}
