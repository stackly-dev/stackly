import AppKit
import Foundation
import StacklyClient
import Synchronization

@testable import StacklyUI

/// A sleep that never waits for real time: each call suspends until the test fires it or
/// the task is cancelled, like `Task.sleep(for:)`.
nonisolated final class ManualSleep: Sendable {
    private struct State {
        var durations: [Duration] = []
        var pending: [Int: CheckedContinuation<Void, any Error>] = [:]
        var cancelled: Set<Int> = []
        var next = 0
    }

    private let state = Mutex(State())

    /// The durations every call asked for, in order.
    var durations: [Duration] { state.withLock { $0.durations } }

    /// How many sleeps wait right now.
    var waiting: Int { state.withLock { $0.pending.count } }

    func sleep(for duration: Duration) async throws {
        let id = state.withLock { state in
            state.durations.append(duration)
            state.next += 1
            return state.next
        }
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let cancelled = state.withLock { state in
                    if state.cancelled.contains(id) { return true }
                    state.pending[id] = continuation
                    return false
                }
                if cancelled { continuation.resume(throwing: CancellationError()) }
            }
        } onCancel: {
            let continuation = state.withLock { state in
                state.cancelled.insert(id)
                return state.pending.removeValue(forKey: id)
            }
            continuation?.resume(throwing: CancellationError())
        }
    }

    /// Suspends the test until `count` sleeps wait.
    func waitUntilSleeping(_ count: Int = 1) async {
        while waiting < count { await Task.yield() }
    }

    /// Ends every waiting sleep, as if its duration passed.
    func fire() {
        let all = state.withLock { state in
            defer { state.pending.removeAll() }
            return Array(state.pending.values)
        }
        for continuation in all { continuation.resume() }
    }
}

/// A `SecretsClient` that records each call and answers from a closure.
nonisolated final class SecretsScript: Sendable {
    private let calls = Mutex<[Call]>([])
    private let answer: @Sendable (Call) -> Result<Reply, ClientError>

    init(_ answer: @escaping @Sendable (Call) -> Result<Reply, ClientError>) {
        self.answer = answer
    }

    /// The calls made so far, in order.
    var made: [Call] { calls.withLock { $0 } }

    var client: SecretsClient {
        SecretsClient { [self] call, _ throws(ClientError) in
            calls.withLock { $0.append(call) }
            return try answer(call).get()
        }
    }

    /// A reference row without a value.
    static func summary(_ reference: String, status: SecretStatus = .present) -> SecretRefSummary {
        SecretRefSummary(
            reference: reference, ownerKind: .user, ownerId: nil, purpose: "user", status: status,
            statusObservedAt: nil, valueOrigin: .entered, valueUpdatedAt: nil, usedBy: [],
            revision: 1)
    }

    /// A `secret.set` reply with `warnings`.
    static func setReply(_ reference: String, warnings: [SecretWarning] = []) -> Reply {
        .secretSet(
            SetSecretReply(summary: summary(reference), warnings: warnings, effects: []))
    }

    /// A `secret.reveal` reply.
    static func revealReply(_ reference: String, _ value: String) -> Reply {
        .secretReveal(RevealSecretReply(reference: reference, value: value))
    }

    /// An error frame body for `code`.
    static func rejection(
        _ code: String, category: ErrorCategory = .conflict, message: String = "something failed",
        details: [ErrorDetail] = [], items: [ErrorItem]? = nil
    ) -> ClientError {
        .rejected(
            ErrorBody(
                code: code, category: category, message: message, retryable: false, resource: nil,
                fields: nil, hint: nil, details: details, operationId: nil, rule: nil,
                items: items))
    }
}

/// A pasteboard that keeps what is written and counts changes like `NSPasteboard`.
final class FakePasteboard: PasteboardStore {
    private(set) var changeCount = 0
    private(set) var strings: [NSPasteboard.PasteboardType: String] = [:]
    private(set) var preparedOptions: [NSPasteboard.ContentsOptions] = []

    func prepareForNewContents(with options: NSPasteboard.ContentsOptions) -> Int {
        preparedOptions.append(options)
        strings.removeAll()
        changeCount += 1
        return changeCount
    }

    func clearContents() -> Int {
        strings.removeAll()
        changeCount += 1
        return changeCount
    }

    func setString(_ string: String, forType dataType: NSPasteboard.PasteboardType) -> Bool {
        strings[dataType] = string
        return true
    }

    /// Whether any type holds `text`.
    func holds(_ text: String) -> Bool {
        strings.values.contains(text)
    }
}
