import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// Why a fixture scenario could not be played.
struct FixtureSettleError: Error, CustomStringConvertible {
    let description: String
}

/// Builds the fixture runtime of `scenario` and waits until its model reaches the
/// scenario's `expectedPresence`.
///
/// Polls the presence every 10 ms; it is a wait for a condition, not a pause that hides
/// timing (spec 0068, AC-3). The limit counts responsive time only (``ResponsiveTimer``).
///
/// - Parameters:
///   - scenario: The scenario to play. It must name an `expectedPresence`.
///   - timeout: The longest wait; 3 seconds by default.
/// - Returns: The runtime, whose model reached the presence.
/// - Throws: ``FixtureSettleError`` with "scenario did not settle" after `timeout`.
@MainActor
func settledRuntime(
    _ scenario: FixtureScenario,
    timeout: Duration = .seconds(3)
) async throws -> ShellRuntime {
    let expected = try #require(
        scenario.expectedPresence, "scenario \(scenario.id.rawValue) has no expected presence")
    let runtime = ShellBootstrap.fixture(scenario)
    var timer = ResponsiveTimer(maxCharge: .milliseconds(60))
    while runtime.model.presence != expected {
        timer.lap()
        guard timer.counted < timeout else {
            let seen = runtime.model.presence
            runtime.willTerminate()
            throw FixtureSettleError(
                description:
                    "scenario did not settle: \(scenario.id.rawValue) expected \(expected), saw \(seen)"
            )
        }
        try await Task.sleep(for: .milliseconds(10))
    }
    return runtime
}
