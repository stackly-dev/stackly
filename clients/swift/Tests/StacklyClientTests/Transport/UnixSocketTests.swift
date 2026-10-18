import Darwin
import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyClient

/// The endpoint checks against the real file system and a real listener:
/// nothing is written to a socket that fails a check (spec 0013 AC-2).
@Suite(.timeLimit(.minutes(1)))
struct UnixSocketTests {
    let root: ShortTempRoot

    init() throws {
        root = try ShortTempRoot()
    }

    func connect(_ endpoint: SocketEndpoint, policy: ConnectPolicy = .sameUser)
        async throws(ClientError)
        -> Int32
    {
        try await UnixSocket.connectVerified(endpoint, policy: policy)
    }

    func untrusted(_ reason: UntrustedReason, at endpoint: SocketEndpoint) -> ClientError {
        .notSent(ClientErrorCatalog.endpointUntrusted(path: endpoint.socketPath, reason: reason))
    }

    @Test func aPrivateSocketInAPrivateFolderConnects() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { _ in }
        let fd = try await connect(root.endpoint)
        #expect(fcntl(fd, F_GETFL) & O_NONBLOCK != 0)
        #expect(fcntl(fd, F_GETFD) & FD_CLOEXEC != 0)
        var on: Int32 = 0
        var length = socklen_t(MemoryLayout<Int32>.size)
        getsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, &length)
        #expect(on != 0)
        close(fd)
    }

    @Test func aSocketOpenToOthersIsUntrustedAndGetsNoByte() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath, mode: 0o666)
        await #expect(throws: untrusted(.groupOrOtherAccess, at: root.endpoint)) {
            try await connect(root.endpoint)
        }
        #expect(server.bytesReceived == 0)
    }

    @Test func aSocketInAnOpenFolderIsUntrusted() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        defer { withExtendedLifetime(server) {} }
        chmod(root.path, 0o755)
        await #expect(throws: untrusted(.parentGroupOrOtherAccess, at: root.endpoint)) {
            try await connect(root.endpoint)
        }
    }

    @Test func aSymlinkToTheRealSocketIsUntrusted() async throws {
        let real = try ShortTempRoot()
        let server = try FakeDaemonServer(path: real.endpoint.socketPath)
        defer { withExtendedLifetime(server) {} }
        symlink(real.endpoint.socketPath, root.endpoint.socketPath)
        await #expect(throws: untrusted(.symlink, at: root.endpoint)) {
            try await connect(root.endpoint)
        }
    }

    @Test func aRegularFileIsNotASocket() async throws {
        FileManager.default.createFile(
            atPath: root.endpoint.socketPath, contents: Data(),
            attributes: [.posixPermissions: 0o600])
        await #expect(throws: untrusted(.notASocket, at: root.endpoint)) {
            try await connect(root.endpoint)
        }
    }

    @Test func anEmptyPathIsEndpointMissing() async throws {
        await #expect(
            throws: ClientError.notSent(
                ClientErrorCatalog.endpointMissing(path: root.endpoint.socketPath))
        ) {
            try await connect(root.endpoint)
        }
    }

    @Test func aStaleSocketIsConnectionRefused() async throws {
        try Self.makeStaleSocket(at: root.endpoint.socketPath)
        await #expect(
            throws: ClientError.notSent(
                ClientErrorCatalog.connectionRefused(path: root.endpoint.socketPath))
        ) {
            try await connect(root.endpoint)
        }
    }

    @Test func aPathAboveSunPathIsRefusedBeforeLstat() async throws {
        let long = SocketEndpoint(
            runtimeRootBytes: Array((root.path + "/" + String(repeating: "a", count: 100)).utf8),
            rootOverride: true)
        let error = await #expect(throws: ClientError.self) { try await connect(long) }
        #expect(error?.body.code == "ipc.io_error")
        #expect(error?.body.detail("op") == "connect")
        #expect(error?.body.detail("kind") == "invalid_filename")
    }

    @Test(
        .evidence(
            covers: ["AC-070"],
            expected:
                "the Swift client refuses a daemon that runs as another user without writing a byte"
        ))
    func aServerOfAnotherUserIsRefusedWithoutAByteWritten() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { _ in }
        let policy = ConnectPolicy(ownerUID: getuid(), peerUID: getuid() + 1)
        await #expect(throws: untrusted(.peerUidMismatch, at: root.endpoint)) {
            try await connect(root.endpoint, policy: policy)
        }
        await server.waitForFinishedConnections(1)
        #expect(server.bytesReceived == 0)
    }

    @Test func folderOfAPathIsEverythingBeforeTheLastSlash() {
        #expect(UnixSocket.folder(of: Array("/a/b/c.sock".utf8)) == Array("/a/b".utf8))
        #expect(UnixSocket.folder(of: Array("/c.sock".utf8)) == Array("/".utf8))
        #expect(UnixSocket.folder(of: Array("c.sock".utf8)) == Array(".".utf8))
    }

    /// A socket file nobody listens on.
    static func makeStaleSocket(at path: String) throws {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        close(fd)
        guard bound == 0 else { throw POSIXError(.EADDRINUSE) }
        chmod(path, 0o600)
    }
}
