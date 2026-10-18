import Darwin
import Dispatch

/// Opens the daemon socket after the trust checks of spec 0006: the path
/// fits `sun_path`, `lstat` of the path and of its folder pass, then
/// `connect`, then the server's uid through `getpeereid`. Nothing is written
/// to a socket that fails any step, and every step runs on one serial queue,
/// never on the main thread or the cooperative pool.
public enum UnixSocket {
    /// The queue every blocking socket call runs on.
    static let queue = DispatchQueue(label: "com.ojiepermana.stackly.client.socket")
    /// The size of `sun_path`, the trailing NUL included.
    static let sunPathSize = MemoryLayout.size(ofValue: sockaddr_un().sun_path)

    /// Connects to `endpoint` after every check passed and returns the
    /// connected, non blocking file descriptor.
    ///
    /// - Parameters:
    ///   - endpoint: Where the socket is.
    ///   - policy: Whose socket and server to accept.
    /// - Throws: ``ClientError/notSent(_:)`` with `ipc.endpoint_missing`,
    ///   `ipc.endpoint_untrusted`, `ipc.connection_refused`, or `ipc.io_error`.
    public static func connectVerified(
        _ endpoint: SocketEndpoint,
        policy: ConnectPolicy
    ) async throws(ClientError) -> Int32 {
        let result = await withCheckedContinuation { continuation in
            queue.async {
                continuation.resume(returning: connectNow(endpoint, policy: policy))
            }
        }
        return try result.get()
    }

    /// The blocking steps of ``connectVerified(_:policy:)``, in order.
    static func connectNow(
        _ endpoint: SocketEndpoint,
        policy: ConnectPolicy
    ) -> Result<Int32, ClientError> {
        let path = endpoint.socketPath
        let bytes = endpoint.socketPathBytes
        guard bytes.count < sunPathSize else {
            return .failure(
                .notSent(ClientErrorCatalog.ioError(op: "connect", errno: ENAMETOOLONG)))
        }
        switch lstatFacts(bytes, socketPath: path) {
        case .failure(let error): return .failure(error)
        case .success(let facts):
            if let reason = EndpointCheck.socketReason(facts, uid: policy.ownerUID) {
                return .failure(
                    .notSent(ClientErrorCatalog.endpointUntrusted(path: path, reason: reason)))
            }
        }
        switch lstatFacts(folder(of: bytes), socketPath: path) {
        case .failure(let error): return .failure(error)
        case .success(let facts):
            if let reason = EndpointCheck.folderReason(facts, uid: policy.ownerUID) {
                return .failure(
                    .notSent(ClientErrorCatalog.endpointUntrusted(path: path, reason: reason)))
            }
        }
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            return .failure(.notSent(ClientErrorCatalog.ioError(op: "connect", errno: errno)))
        }
        if let error = prepare(fd) ?? connect(fd, to: bytes, path: path)
            ?? checkPeer(fd, policy: policy, path: path) ?? setNonBlocking(fd)
        {
            close(fd)
            return .failure(error)
        }
        return .success(fd)
    }

    /// The folder holding the socket path: everything before the last `/`,
    /// `/` for a path in the root, `.` for a bare name.
    static func folder(of path: [UInt8]) -> [UInt8] {
        guard let slash = path.lastIndex(of: UInt8(ascii: "/")) else { return Array(".".utf8) }
        return slash == 0 ? Array("/".utf8) : Array(path[..<slash])
    }

    /// `lstat` of `target`, with `ENOENT` as `ipc.endpoint_missing` for the
    /// socket path and any other errno as `ipc.io_error` with op `stat`.
    static func lstatFacts(_ target: [UInt8], socketPath: String) -> Result<FileStat, ClientError> {
        var info = stat()
        let terminated = target.map { CChar(bitPattern: $0) } + [0]
        let status = terminated.withUnsafeBufferPointer { lstat($0.baseAddress, &info) }
        guard status == 0 else {
            let code = errno
            if code == ENOENT {
                return .failure(.notSent(ClientErrorCatalog.endpointMissing(path: socketPath)))
            }
            return .failure(.notSent(ClientErrorCatalog.ioError(op: "stat", errno: code)))
        }
        return .success(FileStat(info))
    }

    /// Sets `FD_CLOEXEC` and `SO_NOSIGPIPE`.
    static func prepare(_ fd: Int32) -> ClientError? {
        var on: Int32 = 1
        guard fcntl(fd, F_SETFD, FD_CLOEXEC) == 0,
            setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size)) == 0
        else {
            return .notSent(ClientErrorCatalog.ioError(op: "connect", errno: errno))
        }
        return nil
    }

    /// Connects `fd` to the socket at `bytes`.
    static func connect(_ fd: Int32, to bytes: [UInt8], path: String) -> ClientError? {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutableBytes(of: &address.sun_path) { buffer in
            buffer.copyBytes(from: bytes)
        }
        let length = socklen_t(MemoryLayout<sockaddr_un>.size)
        let status = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, length)
            }
        }
        guard status != 0 else { return nil }
        switch errno {
        case ECONNREFUSED: return .notSent(ClientErrorCatalog.connectionRefused(path: path))
        case ENOENT: return .notSent(ClientErrorCatalog.endpointMissing(path: path))
        case let code: return .notSent(ClientErrorCatalog.ioError(op: "connect", errno: code))
        }
    }

    /// Checks that the server process runs as `policy.peerUID`.
    static func checkPeer(_ fd: Int32, policy: ConnectPolicy, path: String) -> ClientError? {
        var uid: uid_t = 0
        var gid: gid_t = 0
        guard getpeereid(fd, &uid, &gid) == 0 else {
            return .notSent(
                ClientErrorCatalog.endpointUntrusted(
                    path: path, reason: .peerCredentialsUnavailable))
        }
        guard uid == policy.peerUID else {
            return .notSent(
                ClientErrorCatalog.endpointUntrusted(path: path, reason: .peerUidMismatch))
        }
        return nil
    }

    /// Makes `fd` non blocking for the channel.
    static func setNonBlocking(_ fd: Int32) -> ClientError? {
        let flags = fcntl(fd, F_GETFL)
        guard flags >= 0, fcntl(fd, F_SETFL, flags | O_NONBLOCK) == 0 else {
            return .notSent(ClientErrorCatalog.ioError(op: "connect", errno: errno))
        }
        return nil
    }
}
