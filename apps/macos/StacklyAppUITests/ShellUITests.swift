import StacklyUI
import StacklyXCTestSupport
import XCTest

/// UI tests of the main window shell on fixture scenarios (spec 0068, AC-14).
@MainActor
final class ShellUITests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
    }

    func testLaunchShowsTheMainWindowOnAScenario() throws {
        try evidence(
            covers: ["AC-024"], expected: "the Debug app opens the main window on a scenario")
        let app = launch(.connectedEmpty)
        XCTAssertTrue(app.windows.firstMatch.exists)
    }
}
