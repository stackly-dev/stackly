import StacklyUI
import XCTest

/// How a UI test starts the Debug app on a fixture scenario (spec 0068).
enum Appearance: String {
    case light
    case dark
}

extension XCTestCase {
    /// Launches the app on `scenario` and waits for the main window, which shows the sidebar
    /// row of Overview, or the element `readyIdentifier` names (the onboarding has no sidebar).
    func launch(
        _ scenario: FixtureScenario.ID, appearance: Appearance = .light,
        readyIdentifier: String = AccessibilityID.Sidebar.row(.overview)
    ) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["STACKLY_UI_SCENARIO"] = scenario.rawValue
        app.launchEnvironment["STACKLY_UI_APPEARANCE"] = appearance.rawValue
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launch()
        let ready = app.descendants(matching: .any)[readyIdentifier]
        XCTAssertTrue(ready.waitForExistence(timeout: 10), "the main window did not appear")
        return app
    }
}
