import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the main window shell on the daemon scenarios (spec 0068, AC-14, AC-18).
///
/// The expected texts are written out here, one row per presence of the table *Tampilan
/// status daemon* of spec 0013. The tests compare the banner title and the banner actions,
/// never the sentence with the time.
@MainActor
final class ShellUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    /// One presence of the table: what the footer reads, the banner title, and its actions.
    struct Expectation {
        let scenario: FixtureScenario.ID
        let footer: String
        let banner: String?
        let actions: [String]

        init(
            _ scenario: FixtureScenario.ID, footer: String, banner: String? = nil,
            actions: [String] = []
        ) {
            self.scenario = scenario
            self.footer = footer
            self.banner = banner
            self.actions = actions
        }
    }

    static let reconnect = "Reconnect"
    static let diagnostics = "Open Diagnostics"
    static let both = [reconnect, diagnostics]
    static let incompatibleTitle = "This version of Stackly can’t talk to stacklyd."

    // MARK: - Footer and banner per scenario

    func testConnectedMixedShowsReadyAndNoBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "a ready daemon shows the running footer and no banner")
        try check(Expectation(.connectedMixed, footer: "stacklyd running"))
    }

    func testConnectedEmptyShowsReadyAndNoBanner() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "an empty workspace shows the running footer and no banner")
        try check(Expectation(.connectedEmpty, footer: "stacklyd running"))
    }

    func testDegradedDaemonShowsOnlyTheFooter() throws {
        try evidence(
            covers: ["AC-024"], expected: "a degraded daemon changes the footer and shows no banner"
        )
        try check(Expectation(.daemonDegraded, footer: "stacklyd degraded"))
    }

    func testUnrecognizedHealthShowsOnlyTheFooter() throws {
        try evidence(
            covers: ["AC-024"], expected: "an unknown health value shows the footer and no banner")
        try check(Expectation(.daemonHealthUnrecognized, footer: "stacklyd status unknown"))
    }

    func testRefreshingShowsTheFooterAndKeepsNoBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "a first snapshot that never ends reads Refreshing…")
        try check(Expectation(.daemonRefreshing, footer: "Refreshing…"))
    }

    func testStartingDaemonShowsAnInformativeBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "a starting daemon shows its banner without actions")
        try check(
            Expectation(
                .daemonStarting, footer: "stacklyd starting…", banner: "stacklyd is starting."))
    }

    func testStoppingDaemonOffersDiagnostics() throws {
        try evidence(
            covers: ["AC-024"], expected: "a stopping daemon shows its banner with Diagnostics")
        try check(
            Expectation(
                .daemonStopping, footer: "stacklyd stopping…",
                banner: "stacklyd is shutting down.", actions: [Self.diagnostics]))
    }

    func testSlowFirstAttemptShowsTheConnectingBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "a first attempt over one second shows the banner")
        try check(
            Expectation(
                .daemonConnecting, footer: "Connecting…", banner: "Connecting to stacklyd…",
                actions: [Self.diagnostics]),
            bannerTimeout: 3)
    }

    func testOfflineDaemonOffersReconnectAndDiagnostics() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "a daemon that went away shows the unreachable banner with Reconnect")
        try check(
            Expectation(
                .daemonOffline, footer: "stacklyd unavailable",
                banner: "Can’t connect to stacklyd.", actions: Self.both))
    }

    func testUnreachableDaemonOffersReconnectAndDiagnostics() throws {
        try evidence(
            covers: ["AC-024"], expected: "an unreachable daemon shows the banner with Reconnect")
        try check(
            Expectation(
                .daemonUnreachable, footer: "stacklyd unavailable",
                banner: "Can’t connect to stacklyd.", actions: Self.both))
    }

    func testMajorMismatchShowsTheIncompatibleBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "a protocol major mismatch shows the critical banner")
        try check(
            Expectation(
                .daemonIncompatibleMajor, footer: "stacklyd incompatible",
                banner: Self.incompatibleTitle, actions: Self.both))
    }

    func testRejectedHandshakeShowsTheIncompatibleBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "a rejected handshake shows the critical banner")
        try check(
            Expectation(
                .daemonIncompatibleRejected, footer: "stacklyd incompatible",
                banner: Self.incompatibleTitle, actions: Self.both))
    }

    func testUntrustedSocketShowsTheSecurityBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "an untrusted socket shows the critical banner")
        try check(
            Expectation(
                .daemonUntrusted, footer: "stacklyd untrusted",
                banner: "Stackly didn’t connect to stacklyd.", actions: Self.both))
    }

    func testRecoveryModeShowsTheRecoveryBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "a registry in recovery shows the critical banner")
        try check(
            Expectation(
                .daemonRecovery, footer: "stacklyd in recovery",
                banner: "Stackly’s registry needs recovery.", actions: [Self.diagnostics]))
    }

    func testMisconfiguredClientShowsTheConfigurationBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "an invalid STACKLY_ROOT shows the critical banner")
        try check(
            Expectation(
                .daemonMisconfigured, footer: "Stackly misconfigured",
                banner: "STACKLY_ROOT is invalid.", actions: [Self.diagnostics]))
    }

    /// Checks one row of the table in the light appearance and attaches the window.
    private func check(_ expectation: Expectation, bannerTimeout: TimeInterval = 5) throws {
        let app = launch(expectation.scenario)
        let footer = app.element(AccessibilityID.DaemonFooter.status)
        XCTAssertTrue(
            footer.waitForLabel("Status: \(expectation.footer)"),
            "the footer reads \(footer.label), expected \(expectation.footer)")

        let banner = app.element(AccessibilityID.DaemonBanner.container)
        if let title = expectation.banner {
            XCTAssertTrue(banner.waitForExistence(timeout: bannerTimeout), "no banner")
            XCTAssertEqual(banner.label, title)
            for action in [Self.reconnect, Self.diagnostics] {
                let button = banner.buttons[action]
                if expectation.actions.contains(action) {
                    XCTAssertTrue(button.exists, "the banner has no \(action) action")
                    XCTAssertTrue(button.isEnabled, "\(action) is disabled")
                } else {
                    XCTAssertFalse(button.exists, "the banner should not offer \(action)")
                }
            }
        } else {
            XCTAssertFalse(banner.exists, "no banner was expected, found \(banner.label)")
        }
        attachScreenshot(
            of: app.mainWindow, scenario: expectation.scenario, appearance: .light,
            part: "overview")
    }

    // MARK: - Last known state offline

    func testOfflineKeepsTheLastKnownInstanceStatuses() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "offline, every instance keeps the status label it had while connected")
        let connected = launch(.connectedMixed)
        select(.instances, in: connected)
        let before = try instanceLabels(in: connected)
        XCTAssertEqual(
            before.count, FixtureScenario(.connectedMixed).instances.count,
            "every instance of the scenario shows a row")
        XCTAssertTrue(
            before.contains { $0.hasPrefix("pg17-dev,") && $0.hasSuffix("Status: Running") },
            "pg17-dev reads Running")
        connected.terminate()

        let offline = launch(.daemonOffline)
        XCTAssertTrue(
            offline.element(AccessibilityID.DaemonFooter.status)
                .waitForLabel("Status: stacklyd unavailable"))
        select(.instances, in: offline)
        let after = try instanceLabels(in: offline)
        XCTAssertEqual(after, before)
        attachScreenshot(
            of: offline.mainWindow, scenario: .daemonOffline, appearance: .light,
            part: "instances")
    }

    /// The spoken labels of the instance rows, without the footer.
    private func instanceLabels(in app: XCUIApplication) throws -> [String] {
        let rows = app.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS ', Status: '"))
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 5), "no instance rows")
        let labels = rows.allElementsBoundByIndex.map(\.label)
        return Array(Set(labels)).sorted()
    }

    // MARK: - Sidebar badge

    func testInstancesBadgeIsLastKnownOffline() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "the Instances badge reads the count, and last known after the daemon goes")
        let count = FixtureScenario(.connectedMixed).instances.count

        let connected = launch(.connectedMixed)
        let live = connected.element(AccessibilityID.Sidebar.row(.instances))
        XCTAssertTrue(live.waitForExistence(timeout: 5))
        XCTAssertEqual(live.value as? String, "\(count) instances")
        attachScreenshot(
            of: connected.mainWindow, scenario: .connectedMixed, appearance: .light,
            part: "overview")
        connected.terminate()

        let offline = launch(.daemonOffline)
        XCTAssertTrue(
            offline.element(AccessibilityID.DaemonFooter.status)
                .waitForLabel("Status: stacklyd unavailable"))
        let stale = offline.element(AccessibilityID.Sidebar.row(.instances))
        XCTAssertEqual(stale.value as? String, "\(count) instances, last known")
        attachScreenshot(
            of: offline.mainWindow, scenario: .daemonOffline, appearance: .light,
            part: "overview")
    }

    // MARK: - Minimum size

    func testWindowStopsAtTheMinimumSize() throws {
        try evidence(
            covers: ["AC-024"], expected: "the main window never gets smaller than 900 × 600 pt")
        let app = launch(.connectedEmpty)
        let window = app.mainWindow
        let corner = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
            .withOffset(CGVector(dx: -2, dy: -2))
        let target = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1))
            .withOffset(CGVector(dx: -800, dy: -600))
        corner.click(forDuration: 0.1, thenDragTo: target)
        let frame = window.frame
        XCTAssertGreaterThanOrEqual(frame.width, 900 - 1)
        XCTAssertGreaterThanOrEqual(frame.height, 600 - 1)
        attachScreenshot(
            of: window, scenario: .connectedEmpty, appearance: .light, part: "overview")
    }
}
