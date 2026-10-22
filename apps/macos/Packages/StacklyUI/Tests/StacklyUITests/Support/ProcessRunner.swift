import Darwin
import Foundation
import Synchronization

/// What a finished child process left behind.
struct ProcessOutcome {
    /// The exit status, or the signal number when a signal ended it.
    let status: Int32
    /// Standard output and standard error, in the order written.
    let output: String
    /// Whether the time limit ran out and the runner killed the process.
    let timedOut: Bool
    /// How long the process ran, from its launch to its exit, measured off the main actor.
    let ranFor: Duration
}

/// Runs a child process to its end without blocking the main actor: the exit arrives
/// through the termination handler, never through `waitUntilExit()`. A process that
/// outlives its time limit is killed with SIGKILL.
///
/// The watchdog and the termination handler run off the main actor. Another test can hold
/// the main actor for seconds in a full run, and that must neither delay the kill nor
/// count as the time the process ran.
enum ProcessRunner {
    /// Shared by the watchdog and the termination handler: the watchdog kills the process
    /// only while it has not ended, so a late watchdog never reaches a reused pid.
    nonisolated final class Watch: Sendable {
        private let state = Mutex((ended: false, killed: false))

        /// Sends SIGKILL to `pid` unless the process already ended.
        func kill(_ pid: pid_t) {
            state.withLock { current in
                guard current.ended == false else { return }
                current.killed = true
                Darwin.kill(pid, SIGKILL)
            }
        }

        /// Records that the process ended.
        func end() {
            state.withLock { $0.ended = true }
        }

        /// Whether the watchdog killed the process.
        var killed: Bool {
            state.withLock { $0.killed }
        }
    }

    /// Runs `executable` and waits for it to exit.
    ///
    /// - Parameters:
    ///   - executable: The binary to run.
    ///   - arguments: Its arguments.
    ///   - environment: Its whole environment; nothing is inherited.
    ///   - timeout: How long it may run before it is killed.
    ///   - onLaunch: Called on the main actor with the pid once the process and its
    ///     watchdog run, so a test can hold the main actor while the process runs.
    /// - Returns: The exit status, the output, whether the limit killed it, and how long
    ///   it ran.
    static func run(
        _ executable: URL,
        arguments: [String],
        environment: [String: String] = ["PATH": "/usr/bin:/bin"],
        timeout: Duration,
        onLaunch: (pid_t) -> Void = { _ in }
    ) async throws -> ProcessOutcome {
        let outputURL = FileManager.default.temporaryDirectory
            .appending(path: "stk-ui-run-\(UUID().uuidString.lowercased()).out")
        let outputPath = outputURL.path(percentEncoded: false)
        guard FileManager.default.createFile(atPath: outputPath, contents: nil) else {
            throw CocoaError(.fileWriteUnknown)
        }
        defer { try? FileManager.default.removeItem(at: outputURL) }
        let sink = try FileHandle(forWritingTo: outputURL)
        defer { try? sink.close() }

        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.environment = environment
        process.standardOutput = sink
        process.standardError = sink
        let watch = Watch()
        let (exits, exit) = AsyncStream.makeStream(
            of: (status: Int32, at: ContinuousClock.Instant).self)
        process.terminationHandler = { finished in
            watch.end()
            exit.yield((finished.terminationStatus, ContinuousClock.now))
            exit.finish()
        }
        let launched = ContinuousClock.now
        try process.run()

        let pid = process.processIdentifier
        let watchdog = Task.detached {
            try await Task.sleep(for: timeout)
            watch.kill(pid)
        }
        onLaunch(pid)
        var status: Int32 = -1
        var ended = launched
        for await exited in exits {
            (status, ended) = (exited.status, exited.at)
            break
        }
        watchdog.cancel()
        try? sink.synchronize()
        let output = (try? String(contentsOf: outputURL, encoding: .utf8)) ?? ""
        return ProcessOutcome(
            status: status, output: output, timedOut: watch.killed, ranFor: ended - launched)
    }
}
