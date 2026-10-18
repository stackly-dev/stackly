import Darwin
import Foundation
import StacklyClient
import Synchronization

/// A scripted daemon on a real Unix socket. It accepts connections on its own
/// thread, counts every byte it receives, and runs a blocking script per
/// connection, so tests can drive any frame order a real daemon might send.
final class FakeDaemonServer: Sendable {
    /// The socket path.
    let path: String
    private let listenFD: Int32
    private let received = Mutex(0)
    private let finished = Mutex(0)
    private let failures = Mutex<[String]>([])

    /// Binds and listens at `path` with `mode`.
    init(path: String, mode: mode_t = 0o600) throws {
        self.path = path
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw FakeServerError.os("socket", errno) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard bound == 0, chmod(path, mode) == 0, listen(fd, 8) == 0 else {
            let code = errno
            Darwin.close(fd)
            throw FakeServerError.os("bind", code)
        }
        listenFD = fd
    }

    deinit {
        Darwin.close(listenFD)
        unlink(path)
    }

    /// Every byte any client wrote to the server.
    var bytesReceived: Int {
        received.withLock { $0 }
    }

    /// Waits until `count` connections were served and closed.
    func waitForFinishedConnections(_ count: Int) async {
        while finished.withLock({ $0 }) < count {
            try? await Task.sleep(for: .milliseconds(5))
        }
    }

    /// What went wrong inside scripts, for the test to check.
    var scriptFailures: [String] {
        failures.withLock { $0 }
    }

    /// Accepts the next connection on a new thread and runs `script` on it,
    /// then closes it. Bytes the script does not read are still counted.
    func serve(_ script: @escaping @Sendable (FakeSession) throws -> Void) {
        let thread = Thread { [self] in
            let fd = accept(listenFD, nil, nil)
            guard fd >= 0 else { return }
            var on: Int32 = 1
            setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
            let session = FakeSession(fd: fd) { count in
                self.received.withLock { $0 += count }
            }
            do {
                try script(session)
            } catch {
                failures.withLock { $0.append("\(error)") }
            }
            session.drainUntilClosed()
            Darwin.close(fd)
            finished.withLock { $0 += 1 }
        }
        thread.start()
    }
}

/// What a fake server could not do.
enum FakeServerError: Error, CustomStringConvertible {
    case os(String, Int32)
    case closed
    case unexpected(String)

    var description: String {
        switch self {
        case .os(let op, let code): "\(op) failed with errno \(code)"
        case .closed: "the client closed the connection"
        case .unexpected(let what): what
        }
    }
}

/// One accepted connection of a ``FakeDaemonServer``, driven by blocking
/// calls on the server's thread.
///
/// `@unchecked Sendable`: only the server thread that runs the script uses
/// it.
final class FakeSession: @unchecked Sendable {
    private let fd: Int32
    private let count: (Int) -> Void
    private var decoder = FrameDecoder(maxLength: ContractLimits.controlFrameMax)
    private var closed = false

    init(fd: Int32, count: @escaping (Int) -> Void) {
        self.fd = fd
        self.count = count
        var timeout = timeval(tv_sec: 5, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
    }

    /// Reads the next frame payload, or `nil` at EOF or after 5 seconds.
    func readPayload() throws -> [UInt8]? {
        while true {
            if let frame = decoder.nextFrame() { return frame }
            guard let chunk = readChunk() else { return nil }
            try decoder.push(chunk)
        }
    }

    /// Reads the next request.
    func readRequest() throws -> Request {
        guard let payload = try readPayload() else { throw FakeServerError.closed }
        return try StacklyJSON.decoder().decode(Request.self, from: Data(payload))
    }

    /// Reads the handshake and accepts it with `limits`.
    @discardableResult
    func acceptHandshake(
        limits: Limits = Samples.limits,
        contractDrift: Bool = false
    ) throws -> Request {
        let request = try readRequest()
        guard case .daemonHandshake = request.call else {
            throw FakeServerError.unexpected("first frame was \(request.call.methodName)")
        }
        try reply(
            to: request,
            with: .daemonHandshake(Samples.accepted(limits: limits, drift: contractDrift)))
        return request
    }

    /// Sends `reply` for `request`.
    func reply(to request: Request, with reply: Reply) throws {
        try send(.response(Response(requestId: request.requestId, reply: reply)))
    }

    /// Sends an error frame for `requestId`.
    func fail(_ requestId: String?, with error: ErrorBody) throws {
        try send(.error(ErrorFrame(requestId: requestId, error: error)))
    }

    /// Sends one daemon frame.
    func send(_ frame: ServerFrame) throws {
        try sendPayload(Array(StacklyJSON.encoder().encode(frame)))
    }

    /// Frames and sends `payload` as it is.
    func sendPayload(_ payload: [UInt8]) throws {
        try sendRaw(FrameEncoder.encode(payload, maxLength: UInt32.max))
    }

    /// Sends raw bytes.
    func sendRaw(_ bytes: [UInt8]) throws {
        var offset = 0
        while offset < bytes.count {
            let written = bytes.withUnsafeBytes {
                Darwin.write(fd, $0.baseAddress.map { $0 + offset }, bytes.count - offset)
            }
            guard written > 0 else { throw FakeServerError.os("write", errno) }
            offset += written
        }
    }

    /// Bytes already buffered or readable now, without waiting.
    func peekAvailable() -> Int {
        var buffer = [UInt8](repeating: 0, count: ContractLimits.readChunk)
        let peeked = buffer.withUnsafeMutableBytes {
            recv(fd, $0.baseAddress, $0.count, MSG_PEEK | MSG_DONTWAIT)
        }
        return decoder.bufferedCount + max(0, peeked)
    }

    /// Closes the connection now.
    func close() {
        guard closed == false else { return }
        closed = true
        shutdown(fd, SHUT_RDWR)
    }

    /// Counts what the client still sends until it closes.
    func drainUntilClosed() {
        while closed == false, readChunk() != nil {}
    }

    private func readChunk() -> [UInt8]? {
        var buffer = [UInt8](repeating: 0, count: ContractLimits.readChunk)
        let read = buffer.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress, $0.count) }
        guard read > 0 else { return nil }
        count(read)
        return Array(buffer.prefix(read))
    }
}

/// Sample contract values for the fake server.
enum Samples {
    /// The limits of the handshake fixture.
    static let limits = Limits(
        maxFrameBytes: 1_048_576, maxLogBatchBytes: 65_536, maxJsonDepth: 32,
        maxInFlightRequests: 32, maxSubscriptions: 8, eventBufferLen: 1_024,
        eventReplayLen: 4_096)

    /// A server instance ID.
    static let serverInstanceId = UUID(uuidString: "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e") ?? UUID()

    /// A handshake reply.
    static func accepted(limits: Limits = limits, drift: Bool = false) -> HandshakeAccepted {
        HandshakeAccepted(
            protocol: ContractInfo.protocolVersion,
            serverInstanceId: serverInstanceId,
            daemon: DaemonIdentity(version: "0.1.0", pid: 501),
            contractDigest: drift ? String(repeating: "0", count: 64) : ContractInfo.digest,
            contractDrift: drift,
            limits: limits,
            head: EventCursor(serverInstanceId: serverInstanceId, sequence: 0))
    }

    /// The client info tests send.
    static let client = ClientInfo(kind: .gui, version: "0.1.0", pid: 42)

    /// A daemon error body.
    static func error(_ code: String, _ category: ErrorCategory, message: String = "refused")
        -> ErrorBody
    {
        ErrorBody(
            code: code, category: category, message: message, retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil)
    }

    /// A call with a unit reply, for tests that only need some call.
    static func unsubscribe(_ id: UInt32) -> Call {
        .eventsUnsubscribe(UnsubscribeParams(subscriptionId: id))
    }
}
