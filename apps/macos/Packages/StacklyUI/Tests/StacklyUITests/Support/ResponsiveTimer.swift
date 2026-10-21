import Foundation

/// Counts the time a polling loop spent while the main actor was responsive.
///
/// Another test can hold the main actor for seconds, for example in an app modal alert
/// run loop. A wall clock deadline would then fail a fixture or snapshot test for a delay
/// it did not cause, so each lap adds at most `maxCharge`, and a stall counts as one lap.
/// The limits of spec 0068 (3 seconds to settle) are limits on this counted time.
struct ResponsiveTimer {
    private let clock = ContinuousClock()
    private var lastLap: ContinuousClock.Instant
    private let maxCharge: Duration

    /// The counted time so far.
    private(set) var counted: Duration = .zero

    /// Starts counting now.
    ///
    /// - Parameter maxCharge: The most one lap adds; a little more than the sleep between
    ///   two laps.
    init(maxCharge: Duration) {
        self.maxCharge = maxCharge
        lastLap = clock.now
    }

    /// Adds the time since the last lap, capped at the maximum charge.
    mutating func lap() {
        let now = clock.now
        counted += min(now - lastLap, maxCharge)
        lastLap = now
    }
}
