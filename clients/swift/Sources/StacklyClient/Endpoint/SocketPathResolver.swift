import Darwin

/// Where the daemon's socket is, as the client computes it.
public struct SocketEndpoint: Hashable, Sendable {
    /// The runtime root, for display.
    public let runtimeRoot: String
    /// The socket path, for display. It may hold replacement characters when
    /// the path is not valid UTF-8; ``socketPathBytes`` holds the real bytes.
    public let socketPath: String
    /// The socket path's raw bytes, which go into `sun_path`.
    public let socketPathBytes: [UInt8]
    /// Whether the path comes from `STACKLY_ROOT`.
    public let rootOverride: Bool

    /// Creates an endpoint from the socket path's raw bytes.
    ///
    /// - Parameters:
    ///   - runtimeRootBytes: The runtime root's raw bytes.
    ///   - rootOverride: Whether the path comes from `STACKLY_ROOT`.
    public init(runtimeRootBytes: [UInt8], rootOverride: Bool) {
        let socketBytes = runtimeRootBytes + Array("/\(SocketPathResolver.socketName)".utf8)
        self.runtimeRoot = String(decoding: runtimeRootBytes, as: UTF8.self)
        self.socketPath = String(decoding: socketBytes, as: UTF8.self)
        self.socketPathBytes = socketBytes
        self.rootOverride = rootOverride
    }
}

/// Why `STACKLY_ROOT` or the user's temporary folder gives no usable socket
/// path. The client never connects while one of these holds.
public enum RootOverrideProblem: Error, Hashable, Sendable {
    /// `STACKLY_ROOT` is set but empty.
    case empty
    /// `STACKLY_ROOT` does not start with `/`.
    case notAbsolute
    /// The folder in `STACKLY_ROOT` does not exist (`ENOENT` or `ENOTDIR`).
    case doesNotExist
    /// `realpath` failed with another errno.
    case unreadable(errno: Int32)
    /// macOS reported no temporary folder for the user.
    case tempDirUnavailable
    /// The runtime root would be longer than ``SocketPathResolver/runtimeRootMax``
    /// bytes, so socket paths would not fit `sun_path`.
    case runtimeRootTooLong(length: Int)
}

/// Derives the daemon socket path with the algorithm of spec 0004, byte for
/// byte the same as the Rust layout. The shared fixture
/// `protocols/fixtures/layout/socket-paths.json` guards it.
public enum SocketPathResolver {
    /// The longest runtime root, in bytes (spec 0004).
    public static let runtimeRootMax = 76
    /// The daemon socket's file name.
    public static let socketName = "stacklyd.sock"

    /// The socket endpoint for `rootOverride` under `userTempDir`.
    ///
    /// 1. The base is `userTempDir` with one trailing `/` dropped.
    /// 2. Without an override, the runtime root is `<base>/stackly`.
    /// 3. With one, it must be absolute; its canonical bytes give
    ///    `<base>/stackly-<fnv8>`.
    /// 4. A runtime root above 76 bytes is refused. The socket is
    ///    `<runtime root>/stacklyd.sock`.
    ///
    /// - Parameters:
    ///   - rootOverride: The value of `STACKLY_ROOT`, or `nil` when it is not
    ///     set.
    ///   - userTempDir: The raw bytes of the user's temporary folder.
    ///   - canonicalize: Resolves the override to its canonical raw bytes.
    /// - Throws: The ``RootOverrideProblem`` that stops the path.
    public static func resolve(
        rootOverride: String?,
        userTempDir: [UInt8],
        canonicalize: ([UInt8]) throws(RootOverrideProblem) -> [UInt8]
    ) throws(RootOverrideProblem) -> SocketEndpoint {
        guard userTempDir.isEmpty == false else { throw .tempDirUnavailable }
        let slash = UInt8(ascii: "/")
        let base = userTempDir.last == slash ? Array(userTempDir.dropLast()) : userTempDir
        let runtimeRoot: [UInt8]
        if let rootOverride {
            guard rootOverride.isEmpty == false else { throw .empty }
            guard rootOverride.utf8.first == slash else { throw .notAbsolute }
            let canonical = try canonicalize(Array(rootOverride.utf8))
            runtimeRoot = base + Array("/stackly-\(FNV1a.short8(canonical))".utf8)
        } else {
            runtimeRoot = base + Array("/stackly".utf8)
        }
        guard runtimeRoot.count <= runtimeRootMax else {
            throw .runtimeRootTooLong(length: runtimeRoot.count)
        }
        return SocketEndpoint(runtimeRootBytes: runtimeRoot, rootOverride: rootOverride != nil)
    }

    /// The socket endpoint of this process: the user's temporary folder from
    /// ``DarwinTempDir`` and `realpath(3)` for the override.
    ///
    /// - Parameter rootOverride: The value of `STACKLY_ROOT`, read once by the
    ///   app at launch, or `nil`.
    /// - Throws: The ``RootOverrideProblem`` that stops the path.
    public static func current(rootOverride: String?) throws(RootOverrideProblem)
        -> SocketEndpoint
    {
        try resolve(
            rootOverride: rootOverride,
            userTempDir: DarwinTempDir.current(),
            canonicalize: realPath
        )
    }

    /// `realpath(3)` of `path`, as raw bytes.
    ///
    /// - Throws: ``RootOverrideProblem/doesNotExist`` for `ENOENT` and
    ///   `ENOTDIR`, ``RootOverrideProblem/unreadable(errno:)`` otherwise.
    static func realPath(_ path: [UInt8]) throws(RootOverrideProblem) -> [UInt8] {
        let terminated = path.map { CChar(bitPattern: $0) } + [0]
        guard let resolved = terminated.withUnsafeBufferPointer({ realpath($0.baseAddress, nil) })
        else {
            let code = errno
            throw code == ENOENT || code == ENOTDIR ? .doesNotExist : .unreadable(errno: code)
        }
        defer { free(resolved) }
        var bytes: [UInt8] = []
        var cursor = resolved
        while cursor.pointee != 0 {
            bytes.append(UInt8(bitPattern: cursor.pointee))
            cursor += 1
        }
        return bytes
    }
}
