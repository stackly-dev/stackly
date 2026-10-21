import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the menu bar against `CommandMap` (spec 0068, AC-16, AC-18; spec 0069).
///
/// Every built row of the map is looked up by its title in the menu the map names, and
/// whether it is on follows the row's scope and requirements. The window starts on
/// Overview, which publishes no page command, so a row of scope `page` is off. The
/// `onboarding` scenario joins the list with feature 16, when it has a window root of its
/// own to show.
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

    func testCommaOpensSettings() throws {
        try evidence(covers: ["REQ-027"], expected: "⌘, opens the Settings window")
        let app = launch(.connectedEmpty)
        app.typeKey(",", modifierFlags: .command)
        let settings = app.windows.matching(NSPredicate(format: "title CONTAINS 'Settings'"))
        XCTAssertTrue(settings.firstMatch.waitForExistence(timeout: 5), "no Settings window")
        attachScreenshot(
            of: settings.firstMatch, scenario: .connectedEmpty, appearance: .light,
            part: "settings")
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
        let footer = app.element(AccessibilityID.DaemonFooter.status)
        XCTAssertTrue(footer.waitForExistence(timeout: 5))
        if connected == false {
            XCTAssertTrue(footer.waitForLabel("Status: stacklyd unavailable"))
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
            defer { app.typeKey(.escape, modifierFlags: []) }
            var menu = bar.menus.firstMatch
            if let submenu = entry.submenu, let submenuTitle = Self.submenuTitles[submenu] {
                let parent = menu.menuItems[submenuTitle]
                XCTAssertTrue(parent.exists, "\(menuTitle) has no \(submenuTitle) submenu")
                parent.click()
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
