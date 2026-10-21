import StacklyUI
import XCTest

/// How a UI test starts the Debug app on a fixture scenario (spec 0068).
enum Appearance: String {
    case light
    case dark
}

extension XCTestCase {
    /// Launches the app on `scenario` and waits for the main window.
    func launch(_ scenario: FixtureScenario.ID, appearance: Appearance = .light) -> XCUIApplication
    {
        let app = XCUIApplication()
        app.launchEnvironment["STACKLY_UI_SCENARIO"] = scenario.rawValue
        app.launchEnvironment["STACKLY_UI_APPEARANCE"] = appearance.rawValue
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launch()
        let overview = app.descendants(matching: .any)[AccessibilityID.Sidebar.row(.overview)]
        XCTAssertTrue(overview.waitForExistence(timeout: 10), "the main window did not appear")
        return app
    }
}
