import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the shell and app commands of spec 0069 (AC-4, AC-7, AC-9, AC-13).
///
/// They lock in what the feature 69 verify run showed with the pointer: View > Overview to
/// Diagnostics pick the section, the shell commands turn off while Settings, the design
/// gallery, the New Instance sheet, or the onboarding is in front, the `app` items stay on,
/// and Window > Stackly brings the main window back after a close or a minimize.
@MainActor
final class ShellCommandsUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    // MARK: - Tests

    func testViewItemsPickEachSection() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "View > Overview to Diagnostics show each section in the main window")
        let app = launch(.connectedMixed)
        for section in Self.sections {
            clickMenuItem(app, "View", section)
            XCTAssertTrue(
                waitForTitle(section, of: app.mainWindow), "View > \(section) did not show it")
        }
    }

    func testViewInstancesFromTheDetailReturnsToTheTable() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "View > Instances while a detail page shows goes back to the table")
        let app = launch(.connectedMixed)
        clickMenuItem(app, "View", "Instances")
        XCTAssertTrue(waitForTitle("Instances", of: app.mainWindow), "Instances did not open")
        let name = app.mainWindow.staticTexts.matching(
            NSPredicate(format: "label == %@ OR value == %@", "pg17-dev", "pg17-dev")
        ).firstMatch
        XCTAssertTrue(name.waitForExistence(timeout: 5), "no row pg17-dev")
        name.click()
        clickMenuItem(app, "Instance", "Open")
        XCTAssertTrue(waitForTitle("pg17-dev", of: app.mainWindow), "the detail did not open")

        clickMenuItem(app, "View", "Instances")
        XCTAssertTrue(
            waitForTitle("Instances", of: app.mainWindow), "View > Instances kept the detail")
    }

    func testSettingsInFrontTurnsOffTheShellCommands() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "with Settings key the shell commands are off and the app items stay on")
        let app = launch(.connectedMixed)
        clickMenuItem(app, "Stackly", "Settings…")
        XCTAssertTrue(app.settingsWindow.waitForExistence(timeout: 5), "Settings did not open")
        assertShellCommandsOff(app, while: "Settings is key")
        assertAppCommandsOn(app, while: "Settings is key")
    }

    func testGalleryInFrontTurnsOffTheShellCommands() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "with the design gallery key the shell commands are off")
        let app = launch(.connectedMixed)
        openGallery(app)
        assertShellCommandsOff(app, while: "the gallery is key")
        assertAppCommandsOn(app, while: "the gallery is key")
    }

    func testNewInstanceSheetTurnsOffTheShellAndInstanceCommands() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "while the New Instance sheet is open the shell and Instance items are off")
        let app = launch(.connectedMixed)
        clickMenuItem(app, "File", "New Instance…")
        XCTAssertTrue(
            app.mainWindow.sheets.firstMatch.waitForExistence(timeout: 5), "no sheet opened")
        assertShellCommandsOff(app, while: "the sheet is open")
        XCTAssertFalse(isOn(app, "Instance", "Start"), "Start is on while the sheet is open")
        XCTAssertTrue(isOn(app, "Stackly", "Settings…"), "Settings… is off with the sheet")
        attachScreenshot(
            of: app.mainWindow, scenario: .connectedMixed, appearance: .light, part: "sheet")
    }

    func testOnboardingTurnsOffTheShellCommands() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "during the onboarding the shell commands are off and the app items on")
        let app = launch(.onboarding, readyText: Self.onboardingFirstStep)
        assertShellCommandsOff(app, while: "the onboarding shows")
        assertAppCommandsOn(app, while: "the onboarding shows")
    }

    func testWelcomeShowsTheOnboardingAndTurnsOffTheShellCommands() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "Help > Welcome to Stackly shows the onboarding in the main window")
        let app = launch(.connectedMixed)
        clickMenuItem(app, "Help", "Welcome to Stackly")
        let step = app.staticTexts[Self.onboardingFirstStep].firstMatch
        XCTAssertTrue(step.waitForExistence(timeout: 5), "the onboarding did not show")
        assertShellCommandsOff(app, while: "the onboarding shows")
        attachScreenshot(
            of: app.mainWindow, scenario: .connectedMixed, appearance: .light, part: "welcome")
    }

    func testWindowStacklyReopensTheMainWindowAfterAClose() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "after the main window closes, Window > Stackly reopens it")
        let app = launch(.connectedMixed)
        app.mainWindow.buttons[XCUIIdentifierCloseWindow].click()
        // The fixture runtime builds the close notice without its panel, so the notice is
        // covered by its unit tests (spec 0013), not here.
        XCTAssertTrue(app.mainWindow.waitForDisappearance(), "the main window did not close")
        XCTAssertFalse(isOn(app, "View", "Overview"), "Overview is on without the window")

        clickMenuItem(app, "Window", "Stackly")
        XCTAssertTrue(app.mainWindow.waitForExistence(timeout: 5), "the window did not reopen")
        XCTAssertTrue(waitForShellCommands(app), "the shell commands did not turn on again")
    }

    func testWindowStacklyBringsBackAMinimizedMainWindow() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "Window > Stackly brings the main window back from the Dock")
        let app = launch(.connectedMixed)
        app.mainWindow.buttons[XCUIIdentifierMinimizeWindow].click()
        XCTAssertTrue(waitForShellCommands(app, on: false), "Overview stays on while minimized")

        clickMenuItem(app, "Window", "Stackly")
        XCTAssertTrue(waitForShellCommands(app), "the window did not come back")
        clickMenuItem(app, "View", "Catalog")
        XCTAssertTrue(waitForTitle("Catalog", of: app.mainWindow), "the window is not usable")
    }

    // MARK: - Helpers

    /// The sidebar sections in View menu order.
    static let sections = [
        "Overview", "Instances", "Environments", "Catalog", "Runtimes", "Operations",
        "Diagnostics",
    ]

    /// The text of the first onboarding step.
    static let onboardingFirstStep = "Step 1 of 5 · Welcome"

    private func assertShellCommandsOff(
        _ app: XCUIApplication, while context: String, line: UInt = #line
    ) {
        XCTAssertFalse(isOn(app, "View", "Overview"), "Overview is on while \(context)", line: line)
        XCTAssertFalse(
            isOn(app, "File", "New Instance…"), "New Instance… is on while \(context)", line: line)
    }

    private func assertAppCommandsOn(
        _ app: XCUIApplication, while context: String, line: UInt = #line
    ) {
        XCTAssertTrue(
            isOn(app, "Stackly", "Settings…"), "Settings… is off while \(context)", line: line)
        XCTAssertTrue(
            isOn(app, "Help", "Welcome to Stackly"), "Welcome is off while \(context)", line: line)
    }

    /// Waits until View > Overview is `on`. The main window becomes key a moment after it
    /// shows, so a single read can come too early.
    private func waitForShellCommands(_ app: XCUIApplication, on: Bool = true) -> Bool {
        for _ in 0..<10 {
            if isOn(app, "View", "Overview") == on { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.5))
        }
        return false
    }

    /// Waits until the window shows the page `title`. macOS joins the subtitle to the title,
    /// as in "Instances – 11 instances", so the subtitle part is not compared.
    private func waitForTitle(_ title: String, of window: XCUIElement) -> Bool {
        let predicate = NSPredicate(
            format: "title == %@ OR title BEGINSWITH %@", title, "\(title) – ")
        let wait = XCTNSPredicateExpectation(predicate: predicate, object: window)
        return XCTWaiter().wait(for: [wait], timeout: 5) == .completed
    }

    /// Opens `menu`, reads whether the item `title` is on, and closes the menu again.
    private func isOn(_ app: XCUIApplication, _ menu: String, _ title: String) -> Bool {
        let bar = app.menuBars.menuBarItems[menu]
        bar.click()
        // Clicking the menu title again closes the open menu without a key press.
        defer { bar.click() }
        let item = bar.menus.firstMatch.menuItems[title]
        XCTAssertTrue(item.waitForExistence(timeout: 5), "\(menu) has no item \(title)")
        return item.isEnabled
    }

    private func clickMenuItem(_ app: XCUIApplication, _ menu: String, _ title: String) {
        let bar = app.menuBars.menuBarItems[menu]
        bar.click()
        let item = bar.menus.firstMatch.menuItems[title]
        XCTAssertTrue(item.waitForExistence(timeout: 5), "\(menu) has no item \(title)")
        XCTAssertTrue(item.isEnabled, "\(menu) > \(title) is off")
        item.click()
    }
}
