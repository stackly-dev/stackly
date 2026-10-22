import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// Holds a reply until the test releases it, so a test decides the order replies arrive in.
@MainActor
final class Gate<Value: Sendable> {
    private var waiters: [CheckedContinuation<Value, Never>] = []

    /// How many callers wait.
    var waiting: Int { waiters.count }

    /// Waits for ``release(_:)``.
    func wait() async -> Value {
        await withCheckedContinuation { waiters.append($0) }
    }

    /// Lets the oldest caller go on with `value`.
    func release(_ value: Value) {
        guard !waiters.isEmpty else { return }
        waiters.removeFirst().resume(returning: value)
    }
}

/// Polls `condition` until it holds, up to two seconds; it is a wait for a state, never a pause.
@MainActor
func settle(
    _ what: String = "the condition", sourceLocation: SourceLocation = #_sourceLocation,
    until condition: () -> Bool
) async {
    for _ in 0..<400 {
        if condition() { return }
        try? await Task.sleep(for: .milliseconds(5))
    }
    Issue.record("\(what) did not hold in time", sourceLocation: sourceLocation)
}

/// A form fixture of the shared contract, `protocols/fixtures/instance/create-form-<name>.json`.
func contractForm(_ name: String) throws -> CreateForm {
    let url = SourceTree.url(of: "protocols/fixtures/instance/create-form-\(name).json")
    return try StacklyJSON.decoder().decode(CreateForm.self, from: Data(contentsOf: url))
}

extension ErrorBody {
    /// A refusal with a code and a message, for the cases that need one.
    static func create(_ code: String, _ message: String = "refused") -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: message, retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
    }
}

/// A duplicate form fixture of the shared contract,
/// `protocols/fixtures/instance/duplicate-form-<name>.json`.
func contractDuplicate(_ name: String) throws -> DuplicateForm {
    let url = SourceTree.url(of: "protocols/fixtures/instance/duplicate-form-\(name).json")
    return try StacklyJSON.decoder().decode(DuplicateForm.self, from: Data(contentsOf: url))
}
