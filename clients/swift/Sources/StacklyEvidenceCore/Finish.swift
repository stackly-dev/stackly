import Foundation

extension Record {
    /// The final record of a test, or nil when the result was not observed and
    /// the record stays `NOT_RUN`.
    ///
    /// A noted `BLOCKED` wins, then any failing issue means `FAIL`, then a
    /// cancelled or skipped test stays `NOT_RUN`, and only then is it `PASS`.
    /// Both the Swift Testing trait and the XCTest helper end their records
    /// here, so the two frameworks cannot drift apart.
    ///
    /// - Parameters:
    ///   - blocked: The reason and owner a test noted, if it was blocked.
    ///   - firstFailure: The description of the first failing issue, if any.
    ///   - unobserved: True when the test was cancelled or skipped without
    ///     `blocked`.
    ///   - actual: The observed value a test noted, if any.
    ///   - now: The finish time.
    package func finished(
        blocked: (reason: String, owner: String)?, firstFailure: String?, unobserved: Bool,
        actual: String?, at now: Date
    ) -> Record? {
        var record = self
        if let blocked {
            record.status = .blocked
            record.actual = nil
            record.reason = blocked.reason
            record.owner = blocked.owner
        } else if let firstFailure {
            record.status = .fail
            record.actual = firstFailure
        } else if unobserved {
            return nil
        } else {
            record.status = .pass
            record.actual = actual ?? Record.defaultActual
        }
        record.finishedAt = now
        return record
    }
}
