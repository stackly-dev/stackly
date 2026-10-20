import Darwin
import Foundation

/// What a finished child process left behind.
struct ProcessOutcome {
    /// The exit status, or the signal number when a signal ended it.
    let status: Int32
    /// Standard output and standard error, in the order written.
    let output: String
    /// Whether the time limit ran out and the runner killed the process.
    let timedOut: Bool
}

/// Runs a child process to its end without blocking the main actor: the exit arrives
/// through the termination handler, never through `waitUntilExit()`. A process that
/// outlives its time limit is killed with SIGKILL.
enum ProcessRunner {
    /// Set by the watchdog when it kills the process.
    private final class TimeoutFlag {
        var didFire = false
    }

    /// Runs `executable` and waits for it to exit.
    ///
    /// - Parameters:
    ///   - executable: The binary to run.
    ///   - arguments: Its arguments.
    ///   - environment: Its whole environment; nothing is inherited.
    ///   - timeout: How long it may run before it is killed.
    /// - Returns: The exit status, the output, and whether the limit killed it.
    static func run(
        _ executable: URL,
        arguments: [String],
        environment: [String: String] = ["PATH": "/usr/bin:/bin"],
        timeout: Duration
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
        let (exits, exit) = AsyncStream.makeStream(of: Int32.self)
        process.terminationHandler = { finished in
            exit.yield(finished.terminationStatus)
            exit.finish()
        }
        try process.run()

        let pid = process.processIdentifier
        let flag = TimeoutFlag()
        let watchdog = Task {
            try await Task.sleep(for: timeout)
            flag.didFire = true
            Darwin.kill(pid, SIGKILL)
        }
        var status: Int32 = -1
        for await code in exits {
            status = code
            break
        }
        watchdog.cancel()
        try? sink.synchronize()
        let output = (try? String(contentsOf: outputURL, encoding: .utf8)) ?? ""
        return ProcessOutcome(status: status, output: output, timedOut: flag.didFire)
    }
}
