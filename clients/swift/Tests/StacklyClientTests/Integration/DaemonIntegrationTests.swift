import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

extension Tag {
    /// Tests that run a real `stacklyd`.
    @Tag static var daemon: Self
}

/// The Swift client against a real `stacklyd` (spec 0013 AC-1, AC-2, AC-5).
/// BLOCKED without the binary or before the daemon serves its socket.
@Suite(.tags(.daemon), .timeLimit(.minutes(1)))
struct DaemonIntegrationTests {
    @Test(
        .evidence(
            covers: ["AC-070"],
            expected:
                "the Swift client computes the daemon's own socket path, verifies it, and completes the handshake"
        ))
    func swiftSocketPathMatchesTheDaemonsAndTheHandshakeCompletes() async throws {
        let daemon = try await SpawnedDaemon()
        let client = ClientInfo(
            kind: .gui, version: "0.1.0", pid: UInt32(getpid()), interactive: true)
        let (connection, accepted) = try await DaemonConnection.open(
            endpoint: daemon.endpoint, policy: .sameUser, client: client)
        #expect(accepted.protocol.major == ContractInfo.protocolVersion.major)
        let reply = try await connection.call(.daemonGetInfo)
        guard case .daemonGetInfo(let info) = reply else {
            Issue.record("daemon.get_info answered \(reply.methodName)")
            return
        }
        #expect(info.socketPath == daemon.endpoint.socketPath)
        #expect(info.roots.rootOverride)
        await connection.close()
        daemon.stop()
    }
}
