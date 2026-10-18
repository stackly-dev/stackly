import Darwin
import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

/// A real `stacklyd` started with `STACKLY_ROOT` at a short private test root,
/// stopped and reaped when the test ends. Tests that need it end as BLOCKED
/// when the binary is missing or does not serve its socket.
final class SpawnedDaemon: @unchecked Sendable {
    /// The root the daemon runs under.
    let root: ShortTempRoot
    /// The socket endpoint the Swift client computes for that root.
    let endpoint: SocketEndpoint
    private let process: Process

    /// Starts the daemon and waits up to 5 seconds for its socket.
    ///
    /// - Throws: The cancellation of a test with the evidence trait, through
    ///   `Evidence.blocked`, when the binary is missing or never serves.
    init() async throws {
        let binary = RepoPaths.daemonBinary
        guard FileManager.default.isExecutableFile(atPath: binary.path(percentEncoded: false))
        else {
            try Evidence.blocked(
                reason:
                    "no stacklyd binary at \(binary.path(percentEncoded: false)); build it with cargo build -p stacklyd",
                owner: "feature 13")
        }
        root = try ShortTempRoot()
        endpoint = try SocketPathResolver.current(rootOverride: root.path)
        process = Process()
        process.executableURL = binary
        process.environment = ["STACKLY_ROOT": root.path, "PATH": "/usr/bin:/bin"]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        for _ in 0..<100 {
            var info = stat()
            if lstat(endpoint.socketPath, &info) == 0 { return }
            if process.isRunning == false { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        stop()
        try Evidence.blocked(
            reason:
                "stacklyd did not serve its socket within 5 seconds; the IPC server comes with feature 6 milestone 4",
            owner: "feature 6")
    }

    deinit {
        stop()
        // The daemon made its runtime root under the user's temporary folder.
        try? FileManager.default.removeItem(atPath: endpoint.runtimeRoot)
    }

    /// Stops the daemon with SIGTERM, then SIGKILL after 5 seconds.
    func stop() {
        guard process.isRunning else { return }
        process.terminate()
        let deadline = Date().addingTimeInterval(5)
        while process.isRunning, Date() < deadline {
            usleep(20_000)
        }
        if process.isRunning {
            kill(process.processIdentifier, SIGKILL)
        }
        process.waitUntilExit()
    }
}
