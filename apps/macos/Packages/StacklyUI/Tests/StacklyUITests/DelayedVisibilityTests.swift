import Synchronization
import Testing

@testable import StacklyUI

/// The delayed spinner of a first load, with a fake sleep instead of real time
/// (spec 0012, State patterns; AC-9).
@Suite(.timeLimit(.minutes(1)))
struct DelayedVisibilityTests {
    @Test func delayIsThreeHundredMillisecondsAndStartsHidden() {
        let visibility = DelayedVisibility()
        #expect(DelayedVisibility.defaultDelay == .milliseconds(300))
        #expect(visibility.delay == .milliseconds(300))
        #expect(visibility.isVisible == false)
    }

    @Test func zeroDelayIsVisibleInTheFirstFrameBeforeAnyTaskRuns() {
        let visibility = DelayedVisibility(delay: .zero)
        #expect(visibility.showsInFirstFrame)
        #expect(visibility.isVisible)
        #expect(DelayedVisibility(delay: .milliseconds(1)).showsInFirstFrame == false)
        #expect(DelayedVisibility(delay: .milliseconds(1)).isVisible == false)
    }

    @Test func zeroDelayNeverSleepsAndStaysVisible() async {
        let sleep = FakeSleep()
        let visibility = DelayedVisibility(delay: .zero, sleep: { try await sleep.sleep(for: $0) })

        await visibility.run()

        #expect(sleep.durations.isEmpty)
        #expect(visibility.isVisible)
    }

    @Test func spinnerNeverShowsWhenDataArrivesWithinTheDelay() async {
        let sleep = FakeSleep()
        let visibility = DelayedVisibility(sleep: { try await sleep.sleep(for: $0) })

        let loading = Task { await visibility.run() }
        await sleep.waitUntilSleeping()
        #expect(sleep.durations == [.milliseconds(300)])
        #expect(visibility.isVisible == false)

        // The data arrived: StateView drops the spinner, which cancels its task.
        loading.cancel()
        await loading.value

        #expect(visibility.isVisible == false)
    }

    @Test func spinnerShowsOnceTheDelayPassed() async {
        let sleep = FakeSleep()
        let visibility = DelayedVisibility(sleep: { try await sleep.sleep(for: $0) })

        let loading = Task { await visibility.run() }
        await sleep.waitUntilSleeping()
        sleep.finish()
        await loading.value

        #expect(visibility.isVisible)
    }

    @Test func enteringLoadingAgainStartsTheDelayOver() async {
        let sleep = FakeSleep()
        let visibility = DelayedVisibility(sleep: { try await sleep.sleep(for: $0) })
        let first = Task { await visibility.run() }
        await sleep.waitUntilSleeping()
        sleep.finish()
        await first.value
        #expect(visibility.isVisible)

        let second = Task { await visibility.run() }
        await sleep.waitUntilSleeping()

        #expect(visibility.isVisible == false)
        #expect(sleep.durations == [.milliseconds(300), .milliseconds(300)])
        second.cancel()
        await second.value
        #expect(visibility.isVisible == false)
    }
}

/// A sleep that never waits for real time: each call records its duration and suspends
/// until the test finishes it or the task is cancelled.
nonisolated private final class FakeSleep: Sendable {
    private struct State {
        var durations: [Duration] = []
        var pending: CheckedContinuation<Void, any Error>?
        var isCancelled = false
    }

    private let state = Mutex(State())

    /// The durations every call asked for, in order.
    var durations: [Duration] {
        state.withLock { $0.durations }
    }

    /// Records `duration` and suspends until ``finish()`` or cancellation, which throws
    /// `CancellationError` like `Task.sleep(for:)`.
    func sleep(for duration: Duration) async throws {
        state.withLock { state in
            state.durations.append(duration)
            state.isCancelled = false
        }
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let isCancelled = state.withLock { state in
                    if state.isCancelled == false {
                        state.pending = continuation
                    }
                    return state.isCancelled
                }
                if isCancelled {
                    continuation.resume(throwing: CancellationError())
                }
            }
        } onCancel: {
            let continuation = state.withLock { state in
                state.isCancelled = true
                defer { state.pending = nil }
                return state.pending
            }
            continuation?.resume(throwing: CancellationError())
        }
    }

    /// Ends the pending sleep, as if its duration passed.
    func finish() {
        let continuation = state.withLock { state in
            defer { state.pending = nil }
            return state.pending
        }
        continuation?.resume()
    }

    /// Waits, by yielding and never by real time, until a sleep is pending.
    func waitUntilSleeping() async {
        while state.withLock({ $0.pending == nil }) {
            await Task.yield()
        }
    }
}
