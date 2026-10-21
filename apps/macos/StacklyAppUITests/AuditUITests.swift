import StacklyUI
import StacklyXCTestSupport
import XCTest

/// Accessibility audits of every sidebar section and every gallery page (spec 0068, AC-17,
/// AC-18), one test per scenario and appearance. An issue that no row of
/// `AuditExceptions` covers fails the test.
@MainActor
final class AuditUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    private static let auditTypes: XCUIAccessibilityAuditType = [
        .contrast, .elementDetection, .hitRegion, .sufficientElementDescription, .action,
        .parentChild,
    ]

    func testConnectedMixedLight() throws {
        try evidence(
            covers: ["NFR-012", "AC-027"], expected: "every section passes the audit")
        try auditSections(.connectedMixed, .light)
    }

    func testConnectedMixedDark() throws {
        try evidence(
            covers: ["NFR-012", "AC-027"], expected: "every section passes the audit in dark")
        try auditSections(.connectedMixed, .dark)
    }

    func testConnectedEmptyLight() throws {
        try evidence(
            covers: ["NFR-012", "AC-027"], expected: "every empty section passes the audit")
        try auditSections(.connectedEmpty, .light)
    }

    func testConnectedEmptyDark() throws {
        try evidence(
            covers: ["NFR-012", "AC-027"], expected: "every empty section passes the audit in dark")
        try auditSections(.connectedEmpty, .dark)
    }

    func testDaemonOfflineLight() throws {
        try evidence(
            covers: ["NFR-012", "AC-027"], expected: "every section passes the audit offline")
        try auditSections(.daemonOffline, .light)
    }

    func testDaemonOfflineDark() throws {
        try evidence(
            covers: ["NFR-012", "AC-027"],
            expected: "every section passes the audit offline in dark")
        try auditSections(.daemonOffline, .dark)
    }

    func testGalleryPagesLight() throws {
        try evidence(
            covers: ["NFR-012", "AC-027"], expected: "every gallery page passes the audit")
        try auditGallery(.light)
    }

    func testGalleryPagesDark() throws {
        try evidence(
            covers: ["NFR-012", "AC-027"], expected: "every gallery page passes the audit in dark")
        try auditGallery(.dark)
    }

    // MARK: - Audits

    private func auditSections(_ scenario: FixtureScenario.ID, _ appearance: Appearance) throws {
        let app = launch(scenario, appearance: appearance)
        if scenario == .daemonOffline {
            XCTAssertTrue(
                app.element(AccessibilityID.DaemonFooter.status)
                    .waitForLabel("Status: stacklyd unavailable"))
        }
        for section in SidebarSection.allCases {
            select(section, in: app)
            attachScreenshot(
                of: app.mainWindow, scenario: scenario, appearance: appearance,
                part: section.rawValue)
            try audit(app)
        }
    }

    private func auditGallery(_ appearance: Appearance) throws {
        let app = launch(.connectedEmpty, appearance: appearance)
        openGallery(app)
        for topic in ["tokens", "status", "states", "confirmation", "secrets", "components"] {
            let row = app.element(AccessibilityID.Gallery.topic(topic))
            XCTAssertTrue(row.waitForExistence(timeout: 5), "no gallery topic \(topic)")
            row.click()
            attachScreenshot(
                of: app.galleryWindow, scenario: .connectedEmpty, appearance: appearance,
                part: topic)
            try audit(app)
        }
    }

    private func audit(_ app: XCUIApplication) throws {
        try app.performAccessibilityAudit(for: Self.auditTypes) { issue in
            AuditExceptions.matches(issue)
        }
    }
}
