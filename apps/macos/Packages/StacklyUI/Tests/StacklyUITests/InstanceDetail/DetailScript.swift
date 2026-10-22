import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization

@testable import StacklyUI

/// A scripted daemon for the detail page: it answers `instance.get_detail` and
/// `instance.get_data_usage` from values a test changes, records what it was asked, and
/// feeds `secret.` events to whoever listens.
nonisolated final class DetailScript: Sendable {
    private struct State {
        var detail: Result<InstanceDetail, ClientError>
        var usage: Result<DataUsage?, ClientError>
        var detailCalls = 0
        var usageCalls: [(refresh: Bool, cachedOnly: Bool)] = []
        var listeners: [AsyncStream<Event>.Continuation] = []
        var detailGate: CheckedContinuation<Void, Never>?
        var holdNextDetail = false
    }

    private let state: Mutex<State>

    init(detail: InstanceDetail, usage: DataUsage? = nil) {
        state = Mutex(State(detail: .success(detail), usage: .success(usage)))
    }

    var detailCalls: Int { state.withLock { $0.detailCalls } }
    var usageCalls: [(refresh: Bool, cachedOnly: Bool)] { state.withLock { $0.usageCalls } }
    var listenerCount: Int { state.withLock { $0.listeners.count } }

    func answer(detail result: Result<InstanceDetail, ClientError>) {
        state.withLock { $0.detail = result }
    }

    func answer(usage result: Result<DataUsage?, ClientError>) {
        state.withLock { $0.usage = result }
    }

    /// Makes the next `instance.get_detail` wait until ``open()``.
    func holdNextDetail() {
        state.withLock { $0.holdNextDetail = true }
    }

    func open() {
        let gate = state.withLock { state -> CheckedContinuation<Void, Never>? in
            defer { state.detailGate = nil }
            return state.detailGate
        }
        gate?.resume()
    }

    func send(_ event: Event) {
        let listeners = state.withLock { $0.listeners }
        for listener in listeners { listener.yield(event) }
    }

    var service: InstanceDetailService {
        InstanceDetailService(
            detail: { [self] _ throws(ClientError) in
                let hold = state.withLock { state -> Bool in
                    state.detailCalls += 1
                    defer { state.holdNextDetail = false }
                    return state.holdNextDetail
                }
                if hold {
                    await withCheckedContinuation { continuation in
                        state.withLock { $0.detailGate = continuation }
                    }
                }
                return try state.withLock { $0.detail }.get()
            },
            usage: { [self] _, refresh, cachedOnly throws(ClientError) in
                state.withLock { $0.usageCalls.append((refresh, cachedOnly)) }
                return try state.withLock { $0.usage }.get()
            },
            secretEvents: { [self] in
                let (stream, continuation) = AsyncStream.makeStream(of: Event.self)
                state.withLock { $0.listeners.append(continuation) }
                return stream
            },
            secrets: SecretsClient { _, _ throws(ClientError) in
                throw .notSent(ClientErrorCatalog.notConnected())
            })
    }
}

extension InstanceDetail {
    /// A copy at another revision.
    func at(revision: UInt64) -> InstanceDetail {
        InstanceDetail(
            instanceId: instanceId, revision: revision, provider: provider, runtime: runtime,
            dataPath: dataPath, startup: startup, secrets: secrets, dependents: dependents,
            connections: connections)
    }
}

extension ErrorBody {
    /// A refusal with `code`, for a scripted daemon.
    static func refusal(_ code: String, message: String = "refused") -> ErrorBody {
        ErrorBody(
            code: code, category: .unavailable, message: message, retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
    }
}
