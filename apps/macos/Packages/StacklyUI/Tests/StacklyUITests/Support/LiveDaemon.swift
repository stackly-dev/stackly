import Darwin
import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

extension Tag {
    /// Tests that run a real `stacklyd`.
    @Tag static var daemon: Self
}

/// A real `stacklyd` for the end to end tests of the shell: it runs with `STACKLY_ROOT`
/// at a short private root, can stop and start again on the same root (so the same
/// socket path), and is stopped and reaped when the test ends.
///
/// The binary is `STACKLY_DAEMON_BIN` (an absolute path) or `target/debug/stacklyd`.
final class LiveDaemon {
    /// The private root, `STACKLY_ROOT` for the daemon and the shell.
    let root: String
    /// The socket endpoint the client computes for that root.
    let endpoint: SocketEndpoint
    private let binary: URL
    private var process: Process?

    /// Makes the root without starting the daemon. A missing binary ends the test: as
    /// `BLOCKED` evidence when the test records evidence, else cancelled.
    ///
    /// - Parameter recordsEvidence: Whether the test carries the evidence trait.
    init(recordsEvidence: Bool) throws {
        binary = Self.binaryURL
        let path = binary.path(percentEncoded: false)
        guard FileManager.default.isExecutableFile(atPath: path) else {
            try Self.unavailable(
                "no stacklyd binary at \(path); build it with cargo build -p stacklyd",
                owner: "feature 13", recordsEvidence: recordsEvidence)
        }
        let name = "/private/tmp/stk-ui-\(UUID().uuidString.prefix(8).lowercased())"
        guard mkdir(name, 0o700) == 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        root = name
        endpoint = try SocketPathResolver.current(rootOverride: name)
    }

    deinit {
        if let process, process.isRunning {
            Darwin.kill(process.processIdentifier, SIGKILL)
            process.waitUntilExit()
        }
        try? FileManager.default.removeItem(atPath: endpoint.runtimeRoot)
        try? FileManager.default.removeItem(atPath: root)
    }

    /// Whether the daemon process runs.
    var isRunning: Bool {
        process?.isRunning == true
    }

    /// The daemon's own log, for a failing test to show.
    var logPath: String {
        "\(root)/stacklyd-test.log"
    }

    /// Starts the daemon and waits up to 5 seconds for its socket.
    func start() async throws {
        let process = Process()
        process.executableURL = binary
        process.environment = ["STACKLY_ROOT": root, "PATH": "/usr/bin:/bin"]
        FileManager.default.createFile(atPath: logPath, contents: nil)
        let log = FileHandle(forWritingAtPath: logPath)
        try log?.seekToEnd()
        process.standardOutput = log ?? FileHandle.nullDevice
        process.standardError = log ?? FileHandle.nullDevice
        try process.run()
        self.process = process
        for _ in 0..<100 {
            var info = stat()
            if lstat(endpoint.socketPath, &info) == 0 { return }
            if process.isRunning == false { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        Issue.record("stacklyd did not serve its socket within 5 seconds; see \(logPath)")
        throw CancellationError()
    }

    /// Stops the daemon with SIGTERM, the way `launchd` or a person stops it, and waits
    /// for it to exit (SIGKILL after 5 seconds).
    func stop() async {
        await end(with: SIGTERM)
    }

    /// Kills the daemon with SIGKILL, as a crash would, leaving its socket file behind.
    func kill() async {
        await end(with: SIGKILL)
    }

    private func end(with signal: Int32) async {
        guard let process, process.isRunning else { return }
        Darwin.kill(process.processIdentifier, signal)
        for _ in 0..<250 where process.isRunning {
            try? await Task.sleep(for: .milliseconds(20))
        }
        if process.isRunning {
            Darwin.kill(process.processIdentifier, SIGKILL)
        }
        process.waitUntilExit()
    }

    private static var binaryURL: URL {
        if let override = ProcessInfo.processInfo.environment["STACKLY_DAEMON_BIN"],
            override.hasPrefix("/")
        {
            return URL(filePath: override)
        }
        return SourceTree.url(of: "target/debug/stacklyd")
    }

    /// Ends the test because a prerequisite is missing.
    static func unavailable(_ reason: String, owner: String, recordsEvidence: Bool) throws
        -> Never
    {
        if recordsEvidence {
            try Evidence.blocked(reason: reason, owner: owner)
        }
        try Test.cancel(Comment(rawValue: reason))
    }
}

extension LiveShellParts {
    /// Stops the model, the sync engine, and the link, so nothing outlives the test.
    func shutDown() async {
        runtime.model.stop()
        await engine.stop()
        await link.stop()
    }
}

/// Waits until `condition` holds, checking every 20 ms, for at most `seconds`. A timeout
/// records an issue naming `what` and the `state` it ended in, and throws, so the test
/// stops there.
///
/// The limit counts responsive time only (``ResponsiveTimer``): another test that holds
/// the main actor for seconds in a full run counts as one check, so the condition is
/// checked again after the hold before the wait can time out.
func waitFor(
    _ what: String,
    seconds: Double,
    state: () -> String = { "" },
    sourceLocation: SourceLocation = #_sourceLocation,
    until condition: () -> Bool
) async throws {
    let limit = Duration.milliseconds(Int64(seconds * 1000))
    var timer = ResponsiveTimer(maxCharge: .milliseconds(70))
    while condition() == false {
        timer.lap()
        guard timer.counted < limit else {
            Issue.record(
                "timed out after \(seconds) s waiting for \(what); \(state())",
                sourceLocation: sourceLocation)
            throw CancellationError()
        }
        try await Task.sleep(for: .milliseconds(20))
    }
}
