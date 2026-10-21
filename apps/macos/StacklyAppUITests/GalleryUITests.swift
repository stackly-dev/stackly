import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the design gallery window (spec 0068, AC-15, AC-18). The gallery is opened
/// from Window > Design Gallery on the `connected-empty` scenario.
///
/// Space on the Show All button needs keyboard focus, which XCUITest on macOS cannot read,
/// so that key stays a manual check.
@MainActor
final class GalleryUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    private func openGallery(topic: String) -> XCUIApplication {
        let app = launch(.connectedEmpty)
        openGallery(app)
        let row = app.element(AccessibilityID.Gallery.topic(topic))
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no gallery topic \(topic)")
        row.click()
        return app
    }

    private func attach(_ app: XCUIApplication, part: String) {
        attachScreenshot(
            of: app.galleryWindow, scenario: .connectedEmpty, appearance: .light, part: part)
    }

    // MARK: - Status

    /// The samples of the Status page, in the order of the page. Each one is mapped with
    /// the same function the app uses, so the test never repeats a label.
    private static let statusSamples: [StatusAppearance] = [
        .instance(lifecycle: .uninitialized, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .initializing, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .stopped, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .starting, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .running, readiness: .awaitingProbe, health: .unknown),
        .instance(
            lifecycle: .running, readiness: .notReady(detail: "Port in use"), health: .unknown),
        .instance(lifecycle: .running, readiness: .notReady(detail: nil), health: .unknown),
        .instance(lifecycle: .running, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .running, readiness: .ready, health: .healthy),
        .instance(lifecycle: .running, readiness: .ready, health: .degraded),
        .instance(lifecycle: .running, readiness: .ready, health: .unhealthy),
        .instance(lifecycle: .recovering, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .stopping, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .failed, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .unknown, readiness: .unknown, health: .unknown),
        .instance(lifecycle: .retained, readiness: .unknown, health: .unknown),
    ]

    func testEveryStatusLabelIsReadAsStatusAndDetail() throws {
        try evidence(
            covers: ["AC-027"], expected: "every status label reads as Status: label, detail")
        let app = openGallery(topic: "status")
        let window = app.galleryWindow
        XCTAssertTrue(
            window.descendants(matching: .any)
                .matching(NSPredicate(format: "label BEGINSWITH 'Status: '")).firstMatch
                .waitForExistence(timeout: 5), "the Status page shows no status label")
        for sample in Self.statusSamples {
            let spoken = spokenStatus(sample)
            let label = window.descendants(matching: .any)
                .matching(NSPredicate(format: "label == %@", spoken)).firstMatch
            XCTAssertTrue(label.exists, "no status label reads \(spoken)")
        }
        attach(app, part: "status")
    }

    // MARK: - Confirmation

    private func confirm(
        _ app: XCUIApplication, trigger: String, key: XCUIKeyboardKey
    ) {
        let window = app.galleryWindow
        window.buttons[trigger].click()
        let sheet = window.sheets.firstMatch
        XCTAssertTrue(sheet.waitForExistence(timeout: 5), "\(trigger) opened no dialog")
        XCTAssertTrue(
            sheet.buttons.count >= 2, "the dialog should have an action and Cancel")
        sheet.typeKey(key, modifierFlags: [])
        XCTAssertTrue(sheet.waitForDisappearance(), "the dialog did not close")
    }

    private func lastResult(_ app: XCUIApplication) -> String {
        app.galleryWindow.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH 'Last result:'")).firstMatch.label
    }

    func testReturnAndEscapeCancelADestructiveDialog() throws {
        try evidence(
            covers: ["AC-027"],
            expected: "Return and Escape both close a destructive dialog without running it")
        let app = openGallery(topic: "confirmation")
        confirm(app, trigger: "Remove pg17-dev…", key: .return)
        XCTAssertTrue(lastResult(app).contains("Nothing ran"), lastResult(app))
        confirm(app, trigger: "Stop mysql-erp…", key: .escape)
        XCTAssertTrue(lastResult(app).contains("Nothing ran"), lastResult(app))
        attach(app, part: "confirmation")
    }

    func testReturnRunsAndEscapeCancelsAStandardDialog() throws {
        try evidence(
            covers: ["AC-027"], expected: "Return runs a standard dialog and Escape cancels it")
        let app = openGallery(topic: "confirmation")
        confirm(app, trigger: "Bring Down erp-development…", key: .return)
        XCTAssertTrue(lastResult(app).contains("Ran “Bring Down”"), lastResult(app))
        confirm(app, trigger: "Bring Down erp-development…", key: .escape)
        XCTAssertTrue(lastResult(app).contains("Nothing ran"), lastResult(app))
        attach(app, part: "confirmation")
    }

    // MARK: - States

    func testShowAllOpensTheRemainingItems() throws {
        try evidence(
            covers: ["AC-027"], expected: "Show All reveals the items a banner hides")
        let app = openGallery(topic: "states")
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
        let app = openGallery(topic: "states")
        let window = app.galleryWindow
        let toggle = window.checkBoxes["Daemon Unreachable"]
        XCTAssertTrue(toggle.waitForExistence(timeout: 5), "no Daemon Unreachable toggle")
        let start = window.buttons["Start"]
        XCTAssertTrue(start.waitForExistence(timeout: 5))
        XCTAssertTrue(start.isEnabled, "Start should be enabled while the daemon is reachable")

        toggle.click()
        XCTAssertFalse(start.isEnabled, "Start should be disabled while the daemon is unreachable")
        XCTAssertTrue(window.buttons["Copy Endpoint"].isEnabled, "Copy Endpoint is never gated")

        let backUp = window.buttons["Back Up Now"]
        XCTAssertFalse(backUp.isEnabled, "Back Up Now is unsupported")
        let reason = window.staticTexts["The Valkey provider doesn’t declare a backup capability."]
        XCTAssertTrue(reason.exists, "the reason of Back Up Now is not written on the page")
        attach(app, part: "states")
    }
}
