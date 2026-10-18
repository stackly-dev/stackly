import Synchronization

/// A clock that moves only when the test calls ``advance(by:)``, so time
/// limits are tested without waiting.
final class ManualClock: Clock, Sendable {
    /// An instant on the manual clock: the time since it started.
    struct Instant: InstantProtocol {
        let offset: Duration

        func advanced(by duration: Duration) -> Instant {
            Instant(offset: offset + duration)
        }

        func duration(to other: Instant) -> Duration {
            other.offset - offset
        }

        static func < (lhs: Instant, rhs: Instant) -> Bool {
            lhs.offset < rhs.offset
        }
    }

    private struct Sleeper {
        let id: Int
        let deadline: Instant
        let continuation: CheckedContinuation<Void, any Error>
    }

    private struct State {
        var now = Instant(offset: .zero)
        var nextID = 0
        var sleepers: [Sleeper] = []
    }

    private let state = Mutex(State())

    var now: Instant {
        state.withLock { $0.now }
    }

    var minimumResolution: Duration {
        .zero
    }

    /// How many sleeps wait for the clock to move.
    var sleeperCount: Int {
        state.withLock { $0.sleepers.count }
    }

    func sleep(until deadline: Instant, tolerance: Duration?) async throws {
        let id = state.withLock { state in
            state.nextID += 1
            return state.nextID
        }
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let due = state.withLock { state -> Bool in
                    if deadline <= state.now || Task.isCancelled { return true }
                    state.sleepers.append(
                        Sleeper(id: id, deadline: deadline, continuation: continuation))
                    return false
                }
                if due {
                    if Task.isCancelled {
                        continuation.resume(throwing: CancellationError())
                    } else {
                        continuation.resume()
                    }
                }
            }
        } onCancel: {
            let cancelled = state.withLock { state -> Sleeper? in
                guard let index = state.sleepers.firstIndex(where: { $0.id == id }) else {
                    return nil
                }
                return state.sleepers.remove(at: index)
            }
            cancelled?.continuation.resume(throwing: CancellationError())
        }
    }

    /// Moves the clock and wakes every sleep that is due.
    func advance(by duration: Duration) {
        let due = state.withLock { state -> [Sleeper] in
            state.now = state.now.advanced(by: duration)
            let now = state.now
            let due = state.sleepers.filter { $0.deadline <= now }
            state.sleepers.removeAll { $0.deadline <= now }
            return due
        }
        for sleeper in due {
            sleeper.continuation.resume()
        }
    }

    /// Waits until `count` sleeps wait for the clock.
    func waitForSleepers(_ count: Int) async {
        while sleeperCount < count {
            await Task.yield()
            try? await Task.sleep(for: .milliseconds(2))
        }
    }
}
