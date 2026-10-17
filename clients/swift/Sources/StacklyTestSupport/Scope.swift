import Foundation
import Synchronization
import Testing

// Keep this file free of the trait's call syntax: `cargo xtask evidence check`
// rejects a file that uses the trait and also cancels a test directly, and
// this is the one place that cancels.

/// The record of the test that is running, and what the test noted so far.
///
/// The issue handler and `Evidence` calls may run on any thread, so the state
/// sits behind a `Mutex`. The `Mutex` lives in a `final class` because it
/// cannot be copied into the escaping closures that share it.
final class Scope: Sendable {
    /// What a test noted before its body ended.
    struct State: Sendable {
        /// The record as it was started, plus the subjects added since.
        var record: Record
        /// The description of every failing issue, oldest first.
        var failures: [String] = []
        /// The observed value from `Evidence.actual`, if any.
        var actual: String?
        /// The reason and owner from `Evidence.blocked`, if it was called.
        var blocked: (reason: String, owner: String)?

        /// The final record, or nil when the result was not observed and the
        /// record stays `NOT_RUN`. A noted `BLOCKED` wins, then any failing
        /// issue means `FAIL`, then a cancelled test stays `NOT_RUN`, and only
        /// then is it `PASS`.
        func finished(cancelled: Bool, at now: Date) -> Record? {
            var record = record
            if let blocked {
                record.status = .blocked
                record.actual = nil
                record.reason = blocked.reason
                record.owner = blocked.owner
            } else if let failure = failures.first {
                record.status = .fail
                record.actual = failure
            } else if cancelled {
                return nil
            } else {
                record.status = .pass
                record.actual = actual ?? Record.defaultActual
            }
            record.finishedAt = now
            return record
        }
    }

    /// Where the records go, or nil when no run is active.
    let target: RunTarget?
    private let state: Mutex<State>
    private let cancelTest: @Sendable (String) throws -> Never

    /// Creates the scope of one test. `cancelTest` defaults to cancelling the
    /// current Swift Testing test; the tests of this target replace it.
    init(
        record: Record, target: RunTarget?,
        cancelTest: @escaping @Sendable (String) throws -> Never = Scope.cancelCurrentTest
    ) {
        self.target = target
        self.state = Mutex(State(record: record))
        self.cancelTest = cancelTest
    }

    /// The kind the test declared.
    var kind: Evidence.Kind {
        state.withLock { $0.record.kind }
    }

    /// A copy of what the test noted so far.
    var snapshot: State {
        state.withLock { $0 }
    }

    /// Notes a failing issue.
    func noteFailure(_ description: String) {
        state.withLock { $0.failures.append(description) }
    }

    /// Notes the observed value of a passing test.
    func noteActual(_ value: String) {
        state.withLock { $0.actual = value }
    }

    /// Notes that a prerequisite is missing.
    func noteBlocked(reason: String, owner: String) {
        state.withLock { $0.blocked = (reason, owner) }
    }

    /// Adds a provider or runtime the test ran against.
    func addSubject(_ subject: Subject) {
        state.withLock { $0.record.subjects.append(subject) }
    }

    /// Cancels the test, so it is reported cancelled rather than passed.
    func cancel(_ reason: String) throws -> Never {
        try cancelTest(reason)
    }

    /// Cancels the current Swift Testing test with `reason` as its comment.
    static func cancelCurrentTest(_ reason: String) throws -> Never {
        try Test.cancel(Comment(rawValue: reason))
    }

    /// Runs `body` as the evidence scope of `test` and returns the record it
    /// left, together with any error that escaped the body.
    ///
    /// It writes the `NOT_RUN` record first, then runs the body with every
    /// failing issue noted and this scope as `Evidence.current`, then writes
    /// the final record. A record whose result was not observed stays
    /// `NOT_RUN`. Problems writing a record are recorded as issues.
    static func run(
        _ scope: Scope, test: Test, testCase: Test.Case?, mismatch: String?,
        body: @Sendable () async throws -> Void
    ) async -> (record: Record, error: (any Error)?) {
        let started = scope.snapshot.record
        if let target = scope.target {
            do {
                try RecordFile.write(started, into: target.directory)
            } catch {
                Issue.record(
                    Comment(
                        rawValue: "evidence: cannot write the NOT_RUN record to "
                            + "\(target.directory.path): \(error)"),
                    sourceLocation: test.sourceLocation)
                return (started, nil)
            }
        }

        let noteFailures = IssueHandlingTrait.compactMapIssues { issue in
            if issue.isFailure {
                scope.noteFailure(String(describing: issue))
            }
            return issue
        }
        var escaped: (any Error)?
        do {
            try await Evidence.$current.withValue(scope) {
                try await noteFailures.provideScope(for: test, testCase: testCase) {
                    if let mismatch {
                        Issue.record(
                            Comment(rawValue: mismatch), sourceLocation: test.sourceLocation)
                        return
                    }
                    try await body()
                }
            }
        } catch {
            escaped = error
        }

        let cancelled = Task.isCancelled
        if let escaped, !cancelled {
            scope.noteFailure("Caught error: \(escaped)")
        }
        guard let finished = scope.snapshot.finished(cancelled: cancelled, at: Date()) else {
            return (started, escaped)
        }
        if let target = scope.target {
            do {
                try RecordFile.write(finished, into: target.directory)
            } catch {
                Issue.record(
                    Comment(
                        rawValue: "evidence: cannot write the final record "
                            + "\(RecordFile.name(of: finished.id)): \(error); it stays NOT_RUN"),
                    sourceLocation: test.sourceLocation)
                return (started, escaped)
            }
        }
        return (finished, escaped)
    }
}

/// Writes raw records to `records/<id>.json` in a run folder.
enum RecordFile {
    /// The name of the raw record file of `id`, with a lowercase UUID like the
    /// Rust guard writes.
    static func name(of id: UUID) -> String {
        "\(id.uuidString.lowercased()).json"
    }

    /// Writes `record` atomically: to `<id>.json.tmp` in the same folder
    /// first, then renamed, so the wrapper never reads a half written file.
    static func write(_ record: Record, into runDirectory: URL) throws {
        let records = runDirectory.appending(path: "records", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: records, withIntermediateDirectories: true)
        let file = records.appending(path: name(of: record.id))
        let temporary = records.appending(path: name(of: record.id) + ".tmp")
        try record.jsonData().write(to: temporary)
        let handle = try FileHandle(forWritingTo: temporary)
        try handle.synchronize()
        try handle.close()
        guard rename(temporary.path, file.path) == 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
    }
}
