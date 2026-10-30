import StacklyUI
import XCTest

extension XCUIApplication {
    /// The main window: the one window with a sidebar that is not the design gallery.
    var mainWindow: XCUIElement {
        windows.matching(NSPredicate(format: "title != %@", "Design Gallery"))
            .containing(.outline, identifier: nil).firstMatch
    }

    /// The design gallery window, found by its title.
    var galleryWindow: XCUIElement {
        windows["Design Gallery"]
    }
}

extension XCUIElement {
    /// Waits until the element no longer exists.
    func waitForDisappearance(timeout: TimeInterval = 5) -> Bool {
        let predicate = NSPredicate(format: "exists == false")
        let wait = XCTNSPredicateExpectation(predicate: predicate, object: self)
        return XCTWaiter().wait(for: [wait], timeout: timeout) == .completed
    }
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

    /// Clicks the row titled `title` in the sidebar of `window`.
    func select(_ title: String, in window: XCUIElement) {
        let row = window.outlines.firstMatch.staticTexts[title]
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no sidebar row \(title)")
        row.click()
    }
}
