import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the main window shell on the daemon scenarios (spec 0068, AC-14, AC-18).
///
/// The expected texts are written out here, one row per presence of the table *Tampilan
/// status daemon* of spec 0013. The tests find the footer, the banner title, and the banner
/// buttons by their visible text, and never compare the sentence with the time.
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
    static let turnOn = "Turn On"
    static let openLoginItems = "Open Login Items"
    static let checkAgain = "Check Again"
    static let startDaemon = "Start stacklyd"
    static let both = [reconnect, diagnostics]
    /// Every action a banner can offer. A banner has exactly the ones its row lists.
    static let allActions = [
        reconnect, diagnostics, turnOn, openLoginItems, checkAgain, startDaemon,
    ]
    static let unavailableFooter = "stacklyd unavailable"
    static let offTitle = "Stackly’s background service is off."
    static let incompatibleTitle = "This version of Stackly can’t talk to stacklyd."
    static let approvalTitle = "Allow Stackly to run in the background."
    static let notFoundTitle = "Stackly can’t find its background service."
    /// Every banner title the table has, so a window without a banner shows none of them.
    static let allTitles = [
        "stacklyd is starting.", "stacklyd is shutting down.", "Connecting to stacklyd…",
        offTitle, approvalTitle, "Stackly’s background service is turned off.",
        "stacklyd isn’t running.", "Stackly couldn’t check its background service.",
        notFoundTitle, incompatibleTitle, "Stackly didn’t connect to stacklyd.",
        "Stackly’s registry needs recovery.", "STACKLY_ROOT is invalid.",
    ]

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

    func testOfflineDaemonOffersTurnOnAndDiagnostics() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "a daemon that went away shows the background service banner")
        try check(
            Expectation(
                .daemonOffline, footer: Self.unavailableFooter,
                banner: Self.offTitle, actions: [Self.turnOn, Self.diagnostics]))
    }

    func testUnreachableDaemonOffersTurnOnAndDiagnostics() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "an unreachable daemon shows the background service banner")
        try check(
            Expectation(
                .daemonUnreachable, footer: Self.unavailableFooter,
                banner: Self.offTitle, actions: [Self.turnOn, Self.diagnostics]))
    }

    // MARK: - The background agent rows (spec 0015, K14 Option A)

    func testRequiresApprovalAgentOffersLoginItemsAndCheckAgain() throws {
        try evidence(
            covers: ["AC-024"], expected: "an agent that needs approval shows its banner row")
        try check(
            Expectation(
                .agentRequiresApproval, footer: Self.unavailableFooter,
                banner: Self.approvalTitle, actions: [Self.openLoginItems, Self.checkAgain]))
    }

    func testDisabledAgentOffersLoginItemsAndCheckAgain() throws {
        try evidence(
            covers: ["AC-024"], expected: "an agent turned off in Login Items shows its banner row")
        try check(
            Expectation(
                .agentDisabled, footer: Self.unavailableFooter,
                banner: "Stackly’s background service is turned off.",
                actions: [Self.openLoginItems, Self.checkAgain]))
    }

    func testEnabledAgentWithoutADaemonOffersStart() throws {
        try evidence(
            covers: ["AC-024"], expected: "an enabled agent and no daemon offers Start stacklyd")
        try check(
            Expectation(
                .agentEnabled, footer: Self.unavailableFooter, banner: "stacklyd isn’t running.",
                actions: [Self.startDaemon, Self.diagnostics]))
    }

    func testNotFoundAgentBeforeTurnOnOffersTurnOn() throws {
        try evidence(
            covers: ["AC-024"], expected: "an agent the OS has not found yet offers Turn On")
        try check(
            Expectation(
                .agentNotFound, footer: Self.unavailableFooter, banner: Self.offTitle,
                actions: [Self.turnOn, Self.diagnostics]))
    }

    func testUnavailableAgentOffersCheckAgain() throws {
        try evidence(
            covers: ["AC-024"], expected: "an agent whose status timed out offers Check Again")
        try check(
            Expectation(
                .agentUnavailable, footer: Self.unavailableFooter,
                banner: "Stackly couldn’t check its background service.",
                actions: [Self.checkAgain, Self.diagnostics]))
    }

    func testTurnOnInTheUnreachableScenarioShowsTheApprovalBanner() throws {
        try evidence(
            covers: ["AC-024"], expected: "Turn On leads to the banner that asks for approval")
        let app = launch(.daemonUnreachable)
        let window = app.mainWindow
        XCTAssertTrue(window.buttons[Self.turnOn].waitForExistence(timeout: 5))
        window.buttons[Self.turnOn].click()
        XCTAssertTrue(
            window.staticTexts[Self.approvalTitle].waitForExistence(timeout: 5),
            "the approval banner did not show")
        try expectActions([Self.openLoginItems, Self.checkAgain], in: window)
        attachScreenshot(
            of: window, scenario: .daemonUnreachable, appearance: .light, part: "overview")
    }

    func testTurnOnWhenTheServiceIsNotFoundShowsTheCriticalBanner() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "Turn On on an agent the OS cannot find shows the critical banner")
        let app = launch(.agentNotFound)
        let window = app.mainWindow
        XCTAssertTrue(window.buttons[Self.turnOn].waitForExistence(timeout: 5))
        window.buttons[Self.turnOn].click()
        XCTAssertTrue(
            window.staticTexts[Self.notFoundTitle].waitForExistence(timeout: 5),
            "the critical banner did not show")
        try expectActions([Self.diagnostics], in: window)
        attachScreenshot(
            of: window, scenario: .agentNotFound, appearance: .light, part: "overview")
    }

    func testStartStacklydShowsTheStartingBannerWithoutActions() throws {
        try evidence(
            covers: ["AC-024"], expected: "Start stacklyd leads to the starting banner")
        let app = launch(.agentEnabled)
        let window = app.mainWindow
        XCTAssertTrue(window.buttons[Self.startDaemon].waitForExistence(timeout: 5))
        window.buttons[Self.startDaemon].click()
        XCTAssertTrue(
            window.staticTexts["stacklyd is starting…"].waitForExistence(timeout: 5),
            "the starting banner did not show")
        try expectActions([], in: window)
        attachScreenshot(
            of: window, scenario: .agentEnabled, appearance: .light, part: "overview")
        // The wait for the daemon ends after 30 seconds; the test does not wait for it.
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
        let window = app.mainWindow
        XCTAssertTrue(
            window.staticTexts[expectation.footer].waitForExistence(timeout: 5),
            "the footer does not read \(expectation.footer)")
        if let title = expectation.banner {
            XCTAssertTrue(
                window.staticTexts[title].waitForExistence(timeout: bannerTimeout),
                "no banner titled \(title)")
            for other in Self.allTitles where other != title {
                XCTAssertFalse(window.staticTexts[other].exists, "a second banner \(other)")
            }
        } else {
            for title in Self.allTitles {
                XCTAssertFalse(window.staticTexts[title].exists, "no banner was expected: \(title)")
            }
        }
        try expectActions(expectation.actions, in: window)
        attachScreenshot(
            of: window, scenario: expectation.scenario, appearance: .light, part: "overview")
    }

    /// Checks that `window` offers exactly `actions` among the banner actions, all enabled.
    private func expectActions(_ actions: [String], in window: XCUIElement) throws {
        for action in Self.allActions {
            let button = window.buttons[action]
            if actions.contains(action) {
                XCTAssertTrue(button.waitForExistence(timeout: 5), "the banner has no \(action)")
                XCTAssertTrue(button.isEnabled, "\(action) is disabled")
            } else {
                XCTAssertFalse(button.exists, "the banner should not offer \(action)")
            }
        }
    }

    // MARK: - Last known state offline

    func testOfflineKeepsTheLastKnownInstanceRows() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "offline, the instance table shows the same rows it had while connected")
        let connected = launch(.connectedMixed)
        select("Instances", in: connected.mainWindow)
        let before = try instanceTexts(in: connected)
        XCTAssertTrue(before.contains("pg17-dev"), "pg17-dev has no row")
        connected.terminate()

        let offline = launch(.daemonOffline)
        XCTAssertTrue(
            offline.mainWindow.staticTexts[Self.unavailableFooter].waitForExistence(timeout: 5))
        select("Instances", in: offline.mainWindow)
        let after = try instanceTexts(in: offline)
        XCTAssertEqual(after, before)
        attachScreenshot(
            of: offline.mainWindow, scenario: .daemonOffline, appearance: .light,
            part: "instances")
    }

    /// The texts of the instance table cells, sorted.
    private func instanceTexts(in app: XCUIApplication) throws -> [String] {
        let table = app.mainWindow.tables.firstMatch
        XCTAssertTrue(table.waitForExistence(timeout: 5), "no instance table")
        XCTAssertTrue(table.staticTexts["pg17-dev"].waitForExistence(timeout: 5), "no rows")
        return table.staticTexts.allElementsBoundByIndex.map(\.label).sorted()
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
