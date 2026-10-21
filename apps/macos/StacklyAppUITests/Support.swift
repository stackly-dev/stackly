import StacklyUI
import XCTest

extension XCUIApplication {
    /// The first element, of any type, with this accessibility identifier.
    func element(_ identifier: String) -> XCUIElement {
        descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    /// The main window, found through its sidebar and not through a title.
    var mainWindow: XCUIElement {
        windows.containing(.any, identifier: AccessibilityID.Sidebar.row(.overview)).firstMatch
    }

    /// The design gallery window, found by its title.
    var galleryWindow: XCUIElement {
        windows["Design Gallery"]
    }
}

extension XCUIElement {
    /// Waits until the label equals `text`.
    func waitForLabel(_ text: String, timeout: TimeInterval = 5) -> Bool {
        let predicate = NSPredicate(format: "label == %@", text)
        let wait = XCTNSPredicateExpectation(predicate: predicate, object: self)
        return XCTWaiter().wait(for: [wait], timeout: timeout) == .completed
    }

    /// Waits until the element no longer exists.
    func waitForDisappearance(timeout: TimeInterval = 5) -> Bool {
        let predicate = NSPredicate(format: "exists == false")
        let wait = XCTNSPredicateExpectation(predicate: predicate, object: self)
        return XCTWaiter().wait(for: [wait], timeout: timeout) == .completed
    }
}

/// What VoiceOver reads for a status label: "Status: <label>", then ", <detail>".
func spokenStatus(_ appearance: StatusAppearance) -> String {
    guard let detail = appearance.detail else { return "Status: \(appearance.label)" }
    return "Status: \(appearance.label), \(detail.text)"
}

extension XCTestCase {
    /// The method name of the running test, such as `testFooter`.
    var methodName: String {
        let parts = name.dropFirst(2).dropLast().split(separator: " ")
        return parts.last.map(String.init) ?? name
    }

    /// Attaches a screenshot of `window` that stays in the result bundle, named
    /// `<method>-<scenario>-<appearance>-<part>` (spec 0068, AC-18). It is only for people
    /// to look at; nothing compares it.
    func attachScreenshot(
        of window: XCUIElement, scenario: FixtureScenario.ID, appearance: Appearance,
        part: String
    ) {
        let attachment = XCTAttachment(screenshot: window.screenshot())
        attachment.name = "\(methodName)-\(scenario.rawValue)-\(appearance.rawValue)-\(part)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    /// Opens the design gallery from Window > Design Gallery and waits for its window.
    func openGallery(_ app: XCUIApplication) {
        app.menuBars.menuBarItems["Window"].click()
        app.menuBars.menuItems["Design Gallery"].click()
        XCTAssertTrue(app.galleryWindow.waitForExistence(timeout: 5), "the gallery did not open")
    }

    /// Chooses a section in the sidebar of the main window.
    func select(_ section: SidebarSection, in app: XCUIApplication) {
        let row = app.element(AccessibilityID.Sidebar.row(section))
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no sidebar row for \(section.rawValue)")
        row.click()
    }
}
