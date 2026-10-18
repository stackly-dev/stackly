import Foundation

/// Paths in the repository, found from this file's path.
enum RepoPaths {
    /// The repository root.
    static var root: URL {
        Fixture.repositoryRoot
    }

    /// The `stacklyd` binary for integration tests: `STACKLY_DAEMON_BIN` when
    /// set (an absolute path), else `target/debug/stacklyd`.
    static var daemonBinary: URL {
        if let override = ProcessInfo.processInfo.environment["STACKLY_DAEMON_BIN"],
            override.hasPrefix("/")
        {
            return URL(filePath: override)
        }
        return root.appending(path: "target/debug/stacklyd")
    }
}
