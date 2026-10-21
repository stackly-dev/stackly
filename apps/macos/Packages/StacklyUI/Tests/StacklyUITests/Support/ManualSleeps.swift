import Foundation

/// A fake `sleep` for tests: every call waits until the test releases it, and the
/// durations asked for are recorded.
actor ManualSleeps {
    private var waiters: [CheckedContinuation<Void, any Error>] = []
    private(set) var durations: [Duration] = []

    /// How many sleeps wait now.
    var waiting: Int { waiters.count }

    /// Waits until released; throws when released with ``cancelAll()``.
    func sleep(_ duration: Duration) async throws {
        durations.append(duration)
        try await withCheckedThrowingContinuation { waiters.append($0) }
    }

    /// Lets every waiting sleep finish normally.
    func releaseAll() {
        let all = waiters
        waiters = []
        for waiter in all { waiter.resume() }
    }

    /// Lets the oldest waiting sleep finish normally.
    func releaseFirst() {
        guard waiters.isEmpty == false else { return }
        waiters.removeFirst().resume()
    }

    /// Makes every waiting sleep throw, as a cancelled `Task.sleep` does.
    func cancelAll() {
        let all = waiters
        waiters = []
        for waiter in all { waiter.resume(throwing: CancellationError()) }
    }

    /// Yields until `count` sleeps wait, so a test continues only once the code under test
    /// has reached its wait.
    func waitUntilWaiting(_ count: Int) async {
        var tries = 0
        while waiters.count < count && tries < 10_000 {
            tries += 1
            await Task.yield()
        }
    }
}
