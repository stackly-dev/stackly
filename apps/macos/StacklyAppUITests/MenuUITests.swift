import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the menu bar against `CommandMap` (spec 0068, AC-16, AC-18; spec 0069).
///
/// Every built row of the map is looked up by its title in the menu the map names, and
/// whether it is on follows the row's scope and requirements. The window starts on
/// Overview, which publishes no page command, so a row of scope `page` is off. Menus are
/// opened and items found with the pointer, never with a key equivalent.
@MainActor
final class MenuUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    func testMenusWhileTheDaemonIsOffline() throws {
        try evidence(
            covers: ["REQ-027"], expected: "offline, Reconnect is on and every built row is found")
        try checkMenus(.daemonOffline, connected: false)
    }

    func testMenusWithAnEmptyWorkspace() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "connected, Reconnect is off and every built row is found")
        try checkMenus(.connectedEmpty, connected: true)
    }

    func testMenusWithMixedInstances() throws {
        try evidence(
            covers: ["REQ-027"],
            expected: "connected, Reconnect is off and every built row is found")
        try checkMenus(.connectedMixed, connected: true)
    }

    func testSettingsMenuItemOpensSettings() throws {
        try evidence(covers: ["REQ-027"], expected: "Stackly > Settings… opens the Settings window")
        let app = launch(.connectedEmpty)
        app.menuBars.menuBarItems["Stackly"].click()
        app.menuBars.menuItems["Settings…"].click()
        XCTAssertTrue(app.settingsWindow.waitForExistence(timeout: 5), "no Settings window")
        attachScreenshot(
            of: app.settingsWindow, scenario: .connectedEmpty, appearance: .light,
            part: "settings")
    }

    func testFooterSettingsButtonOpensSettings() throws {
        try evidence(covers: ["REQ-027"], expected: "the footer Settings button opens Settings")
        let app = launch(.connectedEmpty)
        let button = app.mainWindow.buttons["Settings"]
        XCTAssertTrue(button.waitForExistence(timeout: 5), "no Settings button in the footer")
        button.click()
        XCTAssertTrue(app.settingsWindow.waitForExistence(timeout: 5), "no Settings window")
    }

    // MARK: - Map

    private static let menuTitles: [CommandMenuID: String] = [
        .app: "Stackly", .file: "File", .edit: "Edit", .view: "View",
        .instance: "Instance", .window: "Window", .help: "Help",
    ]

    private static let submenuTitles: [CommandSubmenuID: String] = [
        .find: "Find", .sortInstancesBy: "Sort Instances By",
    ]

    /// Whether the menu item of `entry` is on, from its scope and requirements, on the
    /// Overview page of the main window.
    static func expectedEnabled(_ entry: CommandEntry, connected: Bool) -> Bool {
        switch entry.scope {
        case .app:
            return true
        case .page:
            return false
        case .shell:
            if entry.requirements.contains(.noConnection) && connected { return false }
            if entry.requirements.contains(.daemon) && connected == false { return false }
            let page: Set<CommandRequirement> = [.selection, .detail, .search]
            return entry.requirements.isDisjoint(with: page)
        }
    }

    private func checkMenus(_ scenario: FixtureScenario.ID, connected: Bool) throws {
        let app = launch(scenario)
        if connected == false {
            XCTAssertTrue(
                app.mainWindow.staticTexts["stacklyd unavailable"].waitForExistence(timeout: 5))
        }
        let built = CommandMap.entries.filter { $0.status == .built }
        XCTAssertFalse(built.isEmpty, "the map has no built row")
        for entry in built {
            guard let menuTitle = Self.menuTitles[entry.menu] else {
                XCTFail("no menu title for \(entry.menu)")
                continue
            }
            let bar = app.menuBars.menuBarItems[menuTitle]
            bar.click()
            // Clicking the menu title again closes the open menu without a key press.
            defer { bar.click() }
            var menu = bar.menus.firstMatch
            if let submenu = entry.submenu, let submenuTitle = Self.submenuTitles[submenu] {
                let parent = menu.menuItems[submenuTitle]
                XCTAssertTrue(parent.exists, "\(menuTitle) has no \(submenuTitle) submenu")
                parent.hover()
                menu = parent.menus.firstMatch
            }
            let titles = [entry.title] + (entry.alternateTitle.map { [$0] } ?? [])
            let item = titles.map { menu.menuItems[$0] }.first { $0.exists }
            XCTAssertNotNil(item, "\(menuTitle) has no item titled \(entry.title)")
            guard entry.kind != .system, let item else { continue }
            let expected = Self.expectedEnabled(entry, connected: connected)
            XCTAssertEqual(
                item.isEnabled, expected,
                "\(entry.title) is \(item.isEnabled ? "on" : "off") in \(scenario.rawValue)")
        }
        attachScreenshot(
            of: app.mainWindow, scenario: scenario, appearance: .light, part: "overview")
    }
}
