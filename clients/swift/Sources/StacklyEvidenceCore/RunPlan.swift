import Foundation

/// The run folder and run kind that `cargo xtask evidence run` set.
package struct RunTarget: Equatable, Sendable {
    /// The absolute run folder; records go to its `records/` folder.
    package var directory: URL
    /// The kind of the run.
    package var kind: EvidenceKind

    /// Creates a run target.
    package init(directory: URL, kind: EvidenceKind) {
        self.directory = directory
        self.kind = kind
    }
}

/// A broken evidence argument or environment, which fails the test.
package struct EvidenceSetupError: Error, CustomStringConvertible {
    /// What is wrong, as a lowercase sentence without a trailing period.
    package var description: String

    /// Creates the error.
    package init(description: String) {
        self.description = description
    }
}

/// What the evidence trait does before the test body runs.
package enum RunPlan: Equatable {
    /// The arguments or the environment are wrong: record an issue and skip
    /// the body, writing nothing.
    case reject(String)
    /// A native test outside a native run: cancel the test before writing
    /// anything.
    case cancelNative(String)
    /// Run the body, writing records to `target` when there is one.
    /// `mismatch` is the issue a fixture test records in a native run.
    case run(target: RunTarget?, mismatch: String?)

    /// The absolute path of the run folder. Only the wrapper sets it.
    package static let directoryVariable = "STACKLY_EVIDENCE_DIR"
    /// `fixture` or `native`, the kind of the run. Only the wrapper sets it.
    package static let kindVariable = "STACKLY_EVIDENCE_KIND"

    /// Decides what a test with these trait arguments does, given the process
    /// environment. Tests pass their own dictionary; they never set
    /// environment variables.
    package static func make(
        covers: [String], kind: EvidenceKind, expected: String, environment: [String: String]
    ) -> RunPlan {
        let violations = Rules.argumentViolations(covers: covers, expected: expected)
        if !violations.isEmpty {
            return .reject("evidence: " + violations.joined(separator: "; "))
        }
        let target: RunTarget?
        do {
            target = try runTarget(in: environment)
        } catch {
            return .reject(error.description)
        }
        let runKind =
            target?.kind ?? environment[kindVariable].flatMap { EvidenceKind(rawValue: $0) }
        switch (kind, runKind) {
        case (.native, .native):
            return .run(target: target, mismatch: nil)
        case (.native, _):
            return .cancelNative(
                "this native test runs only in a native run (\(kindVariable)=native), "
                    + "started with `cargo xtask evidence run --kind native`")
        case (.fixture, .native):
            return .run(
                target: target,
                mismatch: "this test is fixture but the run is native: only tests in a target "
                    + "whose name ends with NativeTests run there, and they all declare the "
                    + "native kind")
        case (.fixture, _):
            return .run(target: target, mismatch: nil)
        }
    }

    /// Reads the evidence environment: no run, or a run folder and its kind.
    ///
    /// - Throws: `EvidenceSetupError` when the folder is set but relative or
    ///   missing, or the kind is not `fixture` or `native`.
    package static func runTarget(in environment: [String: String]) throws(EvidenceSetupError)
        -> RunTarget?
    {
        guard let path = environment[directoryVariable] else {
            return nil
        }
        guard path.hasPrefix("/") else {
            throw EvidenceSetupError(
                description: "\(directoryVariable) must be an absolute path, found `\(path)`")
        }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory),
            isDirectory.boolValue
        else {
            throw EvidenceSetupError(
                description: "\(directoryVariable) points at `\(path)`, which is not a folder")
        }
        guard let kind = environment[kindVariable].flatMap({ EvidenceKind(rawValue: $0) }) else {
            throw EvidenceSetupError(
                description:
                    "\(kindVariable) must be `fixture` or `native` when \(directoryVariable) is set"
            )
        }
        return RunTarget(directory: URL(filePath: path, directoryHint: .isDirectory), kind: kind)
    }
}
