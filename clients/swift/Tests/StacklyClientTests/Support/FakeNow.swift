import Foundation
import Synchronization

/// A wall clock the test sets, for `since`, `nextAttempt`, and frame times.
final class FakeNow: Sendable {
    private let value: Mutex<Date>

    init(_ start: Date = Date(timeIntervalSince1970: 1_792_245_720)) {
        value = Mutex(start)
    }

    /// The current fake time.
    var date: Date {
        value.withLock { $0 }
    }

    /// Moves the fake time forward.
    func advance(by seconds: TimeInterval) {
        value.withLock { $0 = $0.addingTimeInterval(seconds) }
    }
}
