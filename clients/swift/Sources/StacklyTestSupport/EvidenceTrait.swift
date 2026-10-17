import Foundation
public import Testing

/// The evidence trait of a test that proves PRD IDs. Create it with the
/// `evidence(covers:kind:expected:)` factory on `Trait`.
///
/// It provides one scope per test function, so a parameterized test leaves
/// one record that is `FAIL` when any case fails. See `Evidence` for the
/// whole flow.
public struct EvidenceTrait: TestTrait, TestScoping {
    /// The PRD IDs the test proves.
    let covers: [String]
    /// Fixture or native.
    let kind: Evidence.Kind
    /// The expected behavior in one sentence.
    let expected: String

    /// Returns this trait for the test function itself and nil for each of
    /// its cases, so one scope and one record cover the whole function.
    public func scopeProvider(for test: Test, testCase: Test.Case?) -> EvidenceTrait? {
        testCase == nil ? self : nil
    }

    /// Cancels a native test outside a native run before anything is
    /// written. Cancelling here, before the test starts, reports it as
    /// skipped with the reason; cancelling inside the scope would hide it.
    public func prepare(for test: Test) async throws {
        if case .cancelNative(let message) = plan() {
            try Scope.cancelCurrentTest(message)
        }
    }

    /// Writes the `NOT_RUN` record, runs the test, then writes its final
    /// record. Wrong arguments or a broken run environment record an issue
    /// at the test and skip its body.
    public func provideScope(
        for test: Test, testCase: Test.Case?,
        performing function: @Sendable () async throws -> Void
    ) async throws {
        guard Evidence.current == nil else {
            Issue.record(
                "evidence: only one evidence trait per test", sourceLocation: test.sourceLocation)
            return
        }
        switch plan() {
        case .reject(let message):
            Issue.record(Comment(rawValue: message), sourceLocation: test.sourceLocation)
        case .cancelNative(let message):
            try Scope.cancelCurrentTest(message)
        case .run(let target, let mismatch):
            let scope = Scope(record: startedRecord(for: test), target: target)
            let outcome = await Scope.run(
                scope, test: test, testCase: testCase, mismatch: mismatch, body: function)
            if let error = outcome.error {
                throw error
            }
        }
    }

    /// What this trait does in the current process environment.
    private func plan() -> RunPlan {
        RunPlan.make(
            covers: covers, kind: kind, expected: expected,
            environment: ProcessInfo.processInfo.environment)
    }

    /// The `NOT_RUN` record of `test`, identified by its module, its name
    /// without module and source location, and where it is written.
    func startedRecord(for test: Test, at now: Date = Date()) -> Record {
        Record(
            schema: Record.schemaName, id: .version7(at: now), covers: covers, kind: kind,
            method: .automated,
            test: TestIdentity(
                language: .swift, package: nil, target: test.sourceLocation.moduleName,
                name: test.id.nameComponents.joined(separator: "/"),
                file: test.sourceLocation.filePath, line: test.sourceLocation.line),
            status: .notRun, expected: expected, actual: nil, reason: nil, owner: nil,
            subjects: [], steps: nil, reviewer: nil, startedAt: now, finishedAt: nil,
            evidencePath: Record.outputLog)
    }
}

extension Trait where Self == EvidenceTrait {
    /// Marks a test as evidence for `covers`.
    ///
    /// - Parameters:
    ///   - covers: The PRD IDs the test proves, for example `["AC-047"]`.
    ///   - kind: `.fixture` (the default) for fakes and fixtures. A native test
    ///     lives in a test target whose name ends with `NativeTests`.
    ///   - expected: The expected behavior in one sentence.
    public static func evidence(
        covers: [String], kind: Evidence.Kind = .fixture, expected: String
    ) -> Self {
        EvidenceTrait(covers: covers, kind: kind, expected: expected)
    }
}
