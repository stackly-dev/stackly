import Foundation
import Synchronization
import Testing

@testable import StacklyTestSupport

/// A fresh folder under the system temporary folder, never Application
/// Support. Call `remove()` in a `defer`.
struct TemporaryFolder {
    let url: URL

    init() throws {
        url = FileManager.default.temporaryDirectory.appending(
            path: "stackly-evidence-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    }

    func remove() {
        try? FileManager.default.removeItem(at: url)
    }
}

/// A value the `@Sendable` closures of a test fill in and the test reads.
final class Captured<Value: Sendable>: Sendable {
    private let value: Mutex<Value>

    init(_ initial: Value) {
        value = Mutex(initial)
    }

    var current: Value {
        value.withLock { $0 }
    }

    func update(_ change: @Sendable (inout Value) -> Void) {
        value.withLock { change(&$0) }
    }
}

/// Stands in for `Test.cancel` inside these tests, so cancelling an inner
/// scope never cancels the test that drives it.
struct InjectedCancel: Error {
    var reason: String
}

/// What one evidence scope left in its run folder.
struct ScopeRun {
    /// The record read back from `records/`.
    var record: Record
    /// Every file name in `records/`.
    var files: [String]
    /// The issues the scope recorded, kept away from the calling test.
    var issues: [String]
    /// The description of the error that escaped the body, if any.
    var escapedError: String?
}

/// Runs `body` as an evidence scope that writes to a fresh run folder, the
/// way the trait does inside a run, and returns what it left on disk.
///
/// Issues the scope records are captured instead of failing the calling
/// test, and `Evidence.blocked` throws `InjectedCancel` instead of
/// cancelling it. `body` gets the run folder.
func runScope(
    declared: Evidence.Kind = .fixture, run: Evidence.Kind? = nil, mismatch: String? = nil,
    body: @Sendable (URL) async throws -> Void
) async throws -> ScopeRun {
    let folder = try TemporaryFolder()
    defer { folder.remove() }
    let test = try #require(Test.current)
    let trait = EvidenceTrait(
        covers: ["REQ-051"], kind: declared, expected: "a scope driven by a test")
    let scope = Scope(
        record: trait.startedRecord(for: test),
        target: RunTarget(directory: folder.url, kind: run ?? declared),
        cancelTest: { reason in throw InjectedCancel(reason: reason) })

    let issues = Captured<[String]>([])
    let escaped = Captured<String?>(nil)
    let captureIssues = IssueHandlingTrait.compactMapIssues { issue in
        issues.update { $0.append(String(describing: issue)) }
        return nil
    }
    try await captureIssues.provideScope(for: test, testCase: nil) {
        let outcome = await Scope.run(
            scope, test: test, testCase: nil, mismatch: mismatch,
            body: { try await body(folder.url) })
        escaped.update { $0 = outcome.error.map { String(describing: $0) } }
    }

    let records = folder.url.appending(path: "records")
    let files = try FileManager.default.contentsOfDirectory(atPath: records.path).sorted()
    let json = try #require(files.first { $0.hasSuffix(".json") })
    let record = try JSONDecoder().decode(
        Record.self, from: Data(contentsOf: records.appending(path: json)))
    return ScopeRun(
        record: record, files: files, issues: issues.current, escapedError: escaped.current)
}
