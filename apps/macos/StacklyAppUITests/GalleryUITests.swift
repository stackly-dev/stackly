import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the design gallery window (spec 0068, AC-15, AC-18). The gallery is opened
/// from Window > Design Gallery on the `connected-empty` scenario, and every control is
/// found by its visible text and clicked with the pointer.
@MainActor
final class GalleryUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    private func openGallery(topic: String) -> XCUIApplication {
        let app = launch(.connectedEmpty)
        openGallery(app)
        select(topic, in: app.galleryWindow)
        return app
    }

    private func attach(_ app: XCUIApplication, part: String) {
        attachScreenshot(
            of: app.galleryWindow, scenario: .connectedEmpty, appearance: .light, part: part)
    }

    // MARK: - Topics

    func testEveryTopicOpensItsPage() throws {
        try evidence(covers: ["AC-027"], expected: "every gallery topic shows its page")
        let app = launch(.connectedEmpty)
        openGallery(app)
        for topic in ["Tokens", "Status", "States", "Confirmation", "Secrets", "Components"] {
            select(topic, in: app.galleryWindow)
            attach(app, part: topic.lowercased())
        }
    }

    // MARK: - Confirmation

    /// Opens the dialog of `trigger` and closes it with the button titled `button`.
    private func answer(_ app: XCUIApplication, trigger: String, with button: String) {
        let window = app.galleryWindow
        window.buttons[trigger].click()
        let sheet = window.sheets.firstMatch
        XCTAssertTrue(sheet.waitForExistence(timeout: 5), "\(trigger) opened no dialog")
        let choice = sheet.buttons[button]
        XCTAssertTrue(choice.exists, "the dialog has no \(button) button")
        choice.click()
        XCTAssertTrue(sheet.waitForDisappearance(), "the dialog did not close")
    }

    /// Whether the Last Result card shows a text that contains `text`.
    private func lastResultContains(_ app: XCUIApplication, _ text: String) -> Bool {
        app.galleryWindow.staticTexts
            .matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
            .waitForExistence(timeout: 5)
    }

    func testCancelClosesADestructiveDialogWithoutRunningIt() throws {
        try evidence(
            covers: ["AC-027"], expected: "Cancel closes a destructive dialog and nothing runs")
        let app = openGallery(topic: "Confirmation")
        answer(app, trigger: "Remove pg17-dev…", with: "Cancel")
        XCTAssertTrue(lastResultContains(app, "Nothing ran"), "the dialog ran its action")
        attach(app, part: "confirmation")
    }

    func testTheActionButtonRunsADestructiveDialog() throws {
        try evidence(covers: ["AC-027"], expected: "the action button runs a destructive dialog")
        let app = openGallery(topic: "Confirmation")
        answer(app, trigger: "Stop mysql-erp…", with: "Stop")
        XCTAssertTrue(lastResultContains(app, "Ran “Stop”"), "the action did not run")
        attach(app, part: "confirmation")
    }

    func testAStandardDialogRunsOrCancels() throws {
        try evidence(
            covers: ["AC-027"], expected: "a standard dialog runs its action or cancels")
        let app = openGallery(topic: "Confirmation")
        answer(app, trigger: "Bring Down erp-development…", with: "Bring Down")
        XCTAssertTrue(lastResultContains(app, "Ran “Bring Down”"), "the action did not run")
        answer(app, trigger: "Bring Down erp-development…", with: "Cancel")
        XCTAssertTrue(lastResultContains(app, "Nothing ran"), "Cancel ran the action")
        attach(app, part: "confirmation")
    }

    // MARK: - States

    func testShowAllOpensTheRemainingItems() throws {
        try evidence(
            covers: ["AC-027"], expected: "Show All reveals the items a banner hides")
        let app = openGallery(topic: "States")
        let showAll = app.galleryWindow.buttons.matching(identifier: "Show All")
        XCTAssertTrue(showAll.firstMatch.waitForExistence(timeout: 5), "no Show All button")
        let before = showAll.count
        showAll.firstMatch.click()
        let gone = NSPredicate { _, _ in showAll.count == before - 1 }
        let wait = XCTNSPredicateExpectation(predicate: gone, object: nil)
        XCTAssertEqual(XCTWaiter().wait(for: [wait], timeout: 5), .completed)
        attach(app, part: "states")
    }

    func testDaemonUnreachableDisablesGatedButtonsWithTheirReason() throws {
        try evidence(
            covers: ["AC-027"],
            expected: "an unreachable daemon disables gated buttons and a reason stays readable")
        let app = openGallery(topic: "States")
        let window = app.galleryWindow
        let toggle = window.checkBoxes["Daemon Unreachable"]
        XCTAssertTrue(toggle.waitForExistence(timeout: 5), "no Daemon Unreachable toggle")
        let start = window.buttons["Start"].firstMatch
        XCTAssertTrue(start.waitForExistence(timeout: 5))
        XCTAssertTrue(start.isEnabled, "Start should be enabled while the daemon is reachable")

        toggle.click()
        XCTAssertFalse(start.isEnabled, "Start should be disabled while the daemon is unreachable")
        XCTAssertTrue(window.buttons["Copy Endpoint"].isEnabled, "Copy Endpoint is never gated")

        let backUp = window.buttons["Back Up Now"].firstMatch
        XCTAssertFalse(backUp.isEnabled, "Back Up Now is unsupported")
        let reason = window.staticTexts["The Valkey provider doesn’t declare a backup capability."]
        XCTAssertTrue(reason.firstMatch.exists, "the reason of Back Up Now is not on the page")
        attach(app, part: "states")
    }
}
