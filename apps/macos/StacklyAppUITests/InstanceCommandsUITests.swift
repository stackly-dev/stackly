import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the page and instance commands of the Instances page (spec 0069 milestone 3,
/// AC-11, AC-12, AC-15; PRD AC-027).
///
/// Each test opens Instances from View > Instances, picks a row by clicking its name, and
/// then reads and clicks the items of the Instance and View menus with the pointer. Whether
/// a menu item is on is compared with the button it stands for on the page (the row action
/// button, the More menu of the detail page), so the menu is shown to follow the button.
/// The fixture `connected-mixed` holds `pg17-dev` running, `mysql-erp` stopped, and `ledger`
/// in an unknown state.
@MainActor
final class InstanceCommandsUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    // MARK: - Tests

    func testInstanceMenuIsOffWithoutASelection() throws {
        try evidence(
            covers: ["AC-027", "REQ-027"],
            expected: "with no row selected every Instance item is off and has no reason")
        let app = openInstances(.connectedMixed)
        for title in Self.instanceItems {
            XCTAssertFalse(isOn(app, "Instance", title), "\(title) is on")
        }
        XCTAssertFalse(menuHasItem(app, "Instance", Self.unknownReason))
        attachScreenshot(
            of: app.mainWindow, scenario: .connectedMixed, appearance: .light, part: "no-selection")
    }

    func testStartAndStopFollowTheRowButton() throws {
        try evidence(
            covers: ["AC-027", "REQ-027"],
            expected:
                "Start is on for a stopped row and Stop for a running one, like the row button")
        let app = openInstances(.connectedMixed)

        selectRow("mysql-erp", in: app)
        XCTAssertTrue(rowButton("mysql-erp", "Start", in: app).isEnabled)
        XCTAssertTrue(isOn(app, "Instance", "Start"), "Start is off for mysql-erp")
        XCTAssertFalse(isOn(app, "Instance", "Stop"), "Stop is on for mysql-erp")

        selectRow("pg17-dev", in: app)
        XCTAssertTrue(rowButton("pg17-dev", "Stop", in: app).isEnabled)
        XCTAssertTrue(isOn(app, "Instance", "Stop"), "Stop is off for pg17-dev")
        XCTAssertFalse(isOn(app, "Instance", "Start"), "Start is on for pg17-dev")
        attachScreenshot(
            of: app.mainWindow, scenario: .connectedMixed, appearance: .light, part: "running")
    }

    func testStopShowsTheRowReasonForAnUnknownInstance() throws {
        try evidence(
            covers: ["AC-027", "REQ-027"],
            expected: "Stop is off for an unknown instance and the menu shows the same reason")
        let app = openInstances(.connectedMixed)
        selectRow("ledger", in: app)
        XCTAssertFalse(rowButton("ledger", "Stop", in: app).isEnabled)
        XCTAssertFalse(isOn(app, "Instance", "Stop"), "Stop is on for ledger")
        XCTAssertTrue(
            menuHasItem(app, "Instance", Self.unknownReason), "the menu has no Stop reason")
    }

    func testOpenAndBackMoveBetweenTableAndDetail() throws {
        try evidence(
            covers: ["AC-027", "REQ-027"],
            expected: "Instance > Open shows the detail page and View > Back returns to the table")
        let app = openInstances(.connectedMixed)
        XCTAssertFalse(isOn(app, "View", "Back"), "Back is on with the table")

        selectRow("pg17-dev", in: app)
        clickMenuItem(app, "Instance", "Open")
        XCTAssertTrue(waitForTitle("pg17-dev", of: app.mainWindow), "the detail did not open")
        XCTAssertFalse(isOn(app, "Instance", "Open"), "Open is on in the detail")
        attachScreenshot(
            of: app.mainWindow, scenario: .connectedMixed, appearance: .light, part: "detail")

        clickMenuItem(app, "View", "Back")
        XCTAssertTrue(
            waitForTitle("Instances", of: app.mainWindow), "Back did not leave the detail")
        XCTAssertFalse(isOn(app, "View", "Back"), "Back stays on after going back")
    }

    func testDetailMenuItemsFollowTheMoreMenu() throws {
        try evidence(
            covers: ["AC-027", "REQ-027"],
            expected: "Copy Connection String and Duplicate… are on exactly when the More items are"
        )
        let app = openInstances(.connectedMixed)
        selectRow("pg17-dev", in: app)
        clickMenuItem(app, "Instance", "Open")
        XCTAssertTrue(waitForTitle("pg17-dev", of: app.mainWindow), "the detail did not open")

        let more = app.mainWindow.toolbars.menuButtons["More"].firstMatch
        XCTAssertTrue(more.waitForExistence(timeout: 5), "the detail has no More menu")
        for title in ["Copy Connection String", "Duplicate…"] {
            more.click()
            let page = more.menuItems[title]
            XCTAssertTrue(page.waitForExistence(timeout: 5), "More has no \(title)")
            let pageEnabled = page.isEnabled
            // Clicking the button again closes its menu without a key press.
            more.click()
            XCTAssertEqual(
                isOn(app, "Instance", title), pageEnabled,
                "\(title) in the Instance menu does not match More")
        }
    }

    func testInspectorItemHidesAndShowsTheInspector() throws {
        try evidence(
            covers: ["AC-027", "REQ-027"],
            expected: "View > Hide Inspector hides it and View > Show Inspector brings it back")
        let app = openInstances(.connectedMixed)
        let placeholder = text("No Selection", in: app.mainWindow)
        XCTAssertTrue(placeholder.waitForExistence(timeout: 5), "the inspector is not open")

        clickMenuItem(app, "View", "Hide Inspector")
        XCTAssertTrue(placeholder.waitForDisappearance(), "the inspector did not close")
        XCTAssertTrue(menuHasItem(app, "View", "Show Inspector"), "the title did not change")

        clickMenuItem(app, "View", "Show Inspector")
        XCTAssertTrue(placeholder.waitForExistence(timeout: 5), "the inspector did not open")
        XCTAssertTrue(menuHasItem(app, "View", "Hide Inspector"), "the title did not change")
    }

    func testSortItemsReorderTheTable() throws {
        try evidence(
            covers: ["AC-027", "REQ-027"],
            expected: "View > Sort Instances By > Descending and Ascending reorder the rows by name"
        )
        let app = openInstances(.connectedMixed)
        XCTAssertTrue(waitForFirstRow("analytics", in: app), "the table does not start by name")

        clickSortItem(app, "Descending")
        XCTAssertTrue(waitForFirstRow("queue", in: app), "Descending did not reorder the rows")
        attachScreenshot(
            of: app.mainWindow, scenario: .connectedMixed, appearance: .light, part: "descending")

        clickSortItem(app, "Ascending")
        XCTAssertTrue(waitForFirstRow("analytics", in: app), "Ascending did not reorder the rows")
    }

    func testOfflineDaemonTurnsOffOnlyTheMutatingItems() throws {
        try evidence(
            covers: ["AC-027", "REQ-027"],
            expected: "offline, Start, Stop, and New Instance… are off while Open still works")
        let app = openInstances(.daemonOffline)
        selectRow("mysql-erp", in: app)
        XCTAssertFalse(rowButton("mysql-erp", "Start", in: app).isEnabled)
        XCTAssertFalse(isOn(app, "Instance", "Start"), "Start is on offline")
        XCTAssertFalse(isOn(app, "Instance", "Stop"), "Stop is on offline")
        XCTAssertFalse(isOn(app, "File", "New Instance…"), "New is on offline")
        XCTAssertTrue(isOn(app, "Instance", "Open"), "Open is off offline")
        XCTAssertTrue(isOn(app, "View", "Hide Inspector"), "Inspector is off")
        attachScreenshot(
            of: app.mainWindow, scenario: .daemonOffline, appearance: .light, part: "selected")
    }

    // MARK: - Helpers

    /// The Instance items that are built, in menu order.
    static let instanceItems = [
        "Open", "Start", "Stop", "Copy Connection String", "Duplicate…",
    ]

    /// The reason `InstanceAction.resolve` gives Stop for an instance in an unknown state.
    static let unknownReason = "Stackly can’t confirm this process."

    /// Launches `scenario` and opens Instances from View > Instances.
    private func openInstances(_ scenario: FixtureScenario.ID) -> XCUIApplication {
        let app = launch(scenario)
        clickMenuItem(app, "View", "Instances")
        XCTAssertTrue(waitForTitle("Instances", of: app.mainWindow), "Instances did not open")
        XCTAssertTrue(table(in: app).waitForExistence(timeout: 5), "Instances has no table")
        return app
    }

    private func table(in app: XCUIApplication) -> XCUIElement {
        // SwiftUI exposes the table as an outline, next to the outline of the sidebar.
        app.mainWindow.outlines.matching(NSPredicate(format: "label != %@", "Sidebar")).firstMatch
    }

    /// The text that shows `text`. SwiftUI puts a cell's text in its value, not its label.
    private func text(_ text: String, in element: XCUIElement) -> XCUIElement {
        element.staticTexts.matching(
            NSPredicate(format: "label == %@ OR value == %@", text, text)
        ).firstMatch
    }

    /// The table row whose name is `name`.
    private func row(_ name: String, in app: XCUIApplication) -> XCUIElement {
        table(in: app).outlineRows.containing(NSPredicate(format: "value == %@", name)).firstMatch
    }

    private func selectRow(_ name: String, in app: XCUIApplication) {
        let cell = text(name, in: row(name, in: app))
        XCTAssertTrue(cell.waitForExistence(timeout: 5), "no row \(name)")
        cell.click()
    }

    /// The action button at the end of the row `name`.
    private func rowButton(_ name: String, _ title: String, in app: XCUIApplication)
        -> XCUIElement
    {
        let button = row(name, in: app).buttons[title]
        XCTAssertTrue(button.waitForExistence(timeout: 5), "row \(name) has no \(title) button")
        return button
    }

    /// Waits until the first row of the table is the instance `name`.
    private func waitForFirstRow(_ name: String, in app: XCUIApplication) -> Bool {
        let first = table(in: app).outlineRows.element(boundBy: 0)
        return text(name, in: first).waitForExistence(timeout: 5)
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

    private func menuHasItem(_ app: XCUIApplication, _ menu: String, _ title: String) -> Bool {
        let bar = app.menuBars.menuBarItems[menu]
        bar.click()
        defer { bar.click() }
        return bar.menus.firstMatch.menuItems[title].exists
    }

    private func clickMenuItem(_ app: XCUIApplication, _ menu: String, _ title: String) {
        let bar = app.menuBars.menuBarItems[menu]
        bar.click()
        let item = bar.menus.firstMatch.menuItems[title]
        XCTAssertTrue(item.waitForExistence(timeout: 5), "\(menu) has no item \(title)")
        XCTAssertTrue(item.isEnabled, "\(menu) > \(title) is off")
        item.click()
    }

    private func clickSortItem(_ app: XCUIApplication, _ title: String) {
        let bar = app.menuBars.menuBarItems["View"]
        bar.click()
        let parent = bar.menus.firstMatch.menuItems["Sort Instances By"]
        XCTAssertTrue(parent.waitForExistence(timeout: 5), "View has no Sort Instances By")
        parent.hover()
        let item = parent.menus.firstMatch.menuItems[title]
        XCTAssertTrue(item.waitForExistence(timeout: 5), "Sort Instances By has no \(title)")
        item.click()
    }
}
