import Foundation
import Synchronization

/// Hands every value to every listener, and the latest value to a new
/// listener first.
final class StateBroadcast<Value: Sendable>: Sendable {
    private struct State {
        var latest: Value?
        var listeners: [UUID: AsyncStream<Value>.Continuation] = [:]
        var finished = false
    }

    private let state = Mutex(State())

    /// A new stream of values, starting with the latest one.
    func stream() -> AsyncStream<Value> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: Value.self, bufferingPolicy: .unbounded)
        let id = UUID()
        let latest = state.withLock { state -> Value? in
            if state.finished { return nil }
            state.listeners[id] = continuation
            return state.latest
        }
        if let latest { continuation.yield(latest) }
        if state.withLock({ $0.finished }) { continuation.finish() }
        continuation.onTermination = { [weak self] _ in
            self?.state.withLock { _ = $0.listeners.removeValue(forKey: id) }
        }
        return stream
    }

    /// The latest value.
    var latest: Value? {
        state.withLock { $0.latest }
    }

    /// Sends `value` to every listener.
    func publish(_ value: Value) {
        let listeners = state.withLock { state in
            state.latest = value
            return Array(state.listeners.values)
        }
        for listener in listeners { listener.yield(value) }
    }

    /// Ends every stream.
    func finish() {
        let listeners = state.withLock { state in
            state.finished = true
            defer { state.listeners.removeAll() }
            return Array(state.listeners.values)
        }
        for listener in listeners { listener.finish() }
    }
}
