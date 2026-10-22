import Darwin
import Foundation
import StacklyClient
import Synchronization

@testable import StacklyUI

/// A daemon on a real Unix socket that answers the snapshot calls of the shell from a
/// fixed set of instances (spec 0013: the fake server; spec 0022 AC-14).
///
/// It serves one connection at a time on its own thread, writes down when it accepted the
/// handshake, and answers every other read the shell makes at startup with an empty page,
/// so the only thing the timing measures is the instance list.
nonisolated final class ScriptedDaemon: Sendable {
    /// The socket endpoint of the private root.
    let endpoint: SocketEndpoint
    private let root: String
    private let listenFD: Int32
    private let handshakeAt = Mutex<Date?>(nil)
    private let instances: [InstanceSummary]
    private let serverInstanceId = UUID()

    /// Binds the socket of a new private root.
    ///
    /// - Parameter instances: The instances `instance.list` answers.
    init(instances: [InstanceSummary]) throws {
        self.instances = instances
        let name = "/private/tmp/stk-ui-\(UUID().uuidString.prefix(8).lowercased())"
        guard mkdir(name, 0o700) == 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        root = name
        let endpoint = try SocketPathResolver.current(rootOverride: name)
        self.endpoint = endpoint
        guard mkdir(endpoint.runtimeRoot, 0o700) == 0 || errno == EEXIST else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutableBytes(of: &address.sun_path) {
            $0.copyBytes(from: endpoint.socketPathBytes)
        }
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard bound == 0, chmod(endpoint.socketPath, 0o600) == 0, listen(fd, 8) == 0 else {
            let code = errno
            Darwin.close(fd)
            throw POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)
        }
        listenFD = fd
    }

    deinit {
        Darwin.close(listenFD)
        unlink(endpoint.socketPath)
        rmdir(endpoint.runtimeRoot)
        rmdir(root)
    }

    /// The private root, for `STACKLY_ROOT` of the shell.
    var rootOverride: String { root }

    /// When the handshake of the latest connection was accepted, or `nil` before.
    var acceptedHandshakeAt: Date? { handshakeAt.withLock { $0 } }

    /// Starts accepting connections.
    func start() {
        let thread = Thread { [self] in
            while true {
                let fd = accept(listenFD, nil, nil)
                guard fd >= 0 else { return }
                var on: Int32 = 1
                setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
                serve(fd)
                Darwin.close(fd)
            }
        }
        thread.start()
    }

    // MARK: - One connection

    private func serve(_ fd: Int32) {
        var decoder = FrameDecoder(maxLength: ContractLimits.controlFrameMax)
        var buffer = [UInt8](repeating: 0, count: ContractLimits.readChunk)
        while true {
            do {
                while let payload = decoder.nextFrame() {
                    let request = try StacklyJSON.decoder().decode(
                        Request.self, from: Data(payload))
                    try answer(request, on: fd)
                }
                let read = buffer.withUnsafeMutableBytes {
                    Darwin.read(fd, $0.baseAddress, $0.count)
                }
                guard read > 0 else { return }
                try decoder.push(Array(buffer.prefix(read)))
            } catch {
                return
            }
        }
    }

    private func answer(_ request: Request, on fd: Int32) throws {
        let reply: Reply
        switch request.call {
        case .daemonHandshake:
            reply = .daemonHandshake(accepted)
            handshakeAt.withLock { $0 = Date() }
        case .daemonGetInfo: reply = .daemonGetInfo(info)
        case .daemonGetHealth: reply = .daemonGetHealth(health)
        case .eventsSubscribe:
            reply = .eventsSubscribe(Subscribed(subscriptionId: 1, head: head))
        case .instanceList: reply = .instanceList(InstancePage(items: instances, next: nil))
        case .operationList: reply = .operationList(OperationPage(items: [], next: nil))
        case .catalogListProviders:
            reply = .catalogListProviders(ListProvidersReply(catalog: .fixture, items: []))
        case .runtimeList:
            reply = .runtimeList(RuntimeListPage(items: [], next: nil, installing: []))
        default:
            try send(
                .error(
                    ErrorFrame(
                        requestId: request.requestId,
                        error: ErrorBody(
                            code: "request.unknown_method", category: .unsupported,
                            message: "unknown method", retryable: false, resource: nil,
                            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
                            items: nil))), on: fd)
            return
        }
        try send(.response(Response(requestId: request.requestId, reply: reply)), on: fd)
    }

    private func send(_ frame: ServerFrame, on fd: Int32) throws {
        let bytes = try FrameEncoder.encode(
            Array(try StacklyJSON.encoder().encode(frame)), maxLength: UInt32.max)
        var offset = 0
        while offset < bytes.count {
            let written = bytes.withUnsafeBytes {
                Darwin.write(fd, $0.baseAddress.map { $0 + offset }, bytes.count - offset)
            }
            guard written > 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
            offset += written
        }
    }

    // MARK: - Values

    private var head: EventCursor {
        EventCursor(serverInstanceId: serverInstanceId, sequence: 0)
    }

    private var accepted: HandshakeAccepted {
        HandshakeAccepted(
            protocol: ContractInfo.protocolVersion, serverInstanceId: serverInstanceId,
            daemon: DaemonIdentity(version: "0.1.0", pid: 501),
            contractDigest: ContractInfo.digest, contractDrift: false,
            limits: Limits(
                maxFrameBytes: 1_048_576, maxLogBatchBytes: 65_536, maxJsonDepth: 32,
                maxInFlightRequests: 32, maxSubscriptions: 8, eventBufferLen: 1_024,
                eventReplayLen: 4_096),
            head: head)
    }

    private var info: DaemonInfo {
        DaemonInfo(
            version: "0.1.0", protocol: ContractInfo.protocolVersion,
            contractDigest: ContractInfo.digest, serverInstanceId: serverInstanceId, pid: 501,
            uid: 501, startedAt: FixtureClock.now,
            roots: DaemonRoots(
                data: "\(root)/data", logs: "\(root)/logs", cache: "\(root)/cache",
                runtime: endpoint.runtimeRoot, rootOverride: true),
            socketPath: endpoint.socketPath, registrySchema: 2, secretStore: nil, launch: nil)
    }

    private var health: DaemonHealth {
        DaemonHealth(
            status: .ready, subsystems: [],
            counters: DaemonCounters(
                connectionsOpen: 1, connectionsTotal: 1, connectionsRejected: 0,
                framesRejected: 0, eventsPublished: 0, eventsDropped: 0, logLinesDropped: 0,
                socketRebinds: 0),
            recovery: nil)
    }
}
