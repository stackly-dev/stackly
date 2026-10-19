import Foundation
import StacklyClient
import Synchronization
import Testing

/// The link's status, waits, and heartbeat against a fake daemon, a manual
/// clock, and a fixed wall clock (spec 0013 AC-7, AC-11).
@Suite(.timeLimit(.minutes(1)))
struct DaemonLinkTests {
    static let now = Date(timeIntervalSince1970: 1_792_245_720)

    let root: ShortTempRoot
    let clock = ManualClock()

    init() throws {
        root = try ShortTempRoot()
    }

    func makeLink(
        endpoint: Result<SocketEndpoint, RootOverrideProblem>? = nil,
        version: String = "0.1.0"
    ) -> DaemonLink {
        DaemonLink(
            endpoint: endpoint ?? .success(root.endpoint), clientVersion: version, pid: 42,
            clock: clock, now: { Self.now })
    }

    /// The seconds between now and the state's next attempt.
    static func wait(_ state: ConnectionState?) -> Double? {
        switch state {
        case .unreachable(_, _, let next), .incompatible(_, _, let next),
            .untrusted(_, _, let next):
            next.timeIntervalSince(now)
        default:
            nil
        }
    }

    @Test func unreachableWaitsGrowThenStayAtFiveSeconds() async throws {
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        var waits: [Double] = []
        for _ in 0..<6 {
            let state = await states.next { if case .unreachable = $0 { true } else { false } }
            let wait = try #require(Self.wait(state))
            waits.append(wait)
            await clock.waitForSleepers(1)
            clock.advance(by: .milliseconds(Int64(wait * 1000)))
        }
        #expect(waits == [0.5, 1, 2, 4, 5, 5])
        if case .unreachable(let error, let since, _) = await states.latest() {
            #expect(error.code == "ipc.endpoint_missing")
            #expect(since == Self.now)
        }
        await link.stop()
    }

    @Test func retryNowStartsAnAttemptWithoutChangingTheOrder() async throws {
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        _ = await states.next { Self.wait($0) == 0.5 }
        await clock.waitForSleepers(1)
        await link.retryNow()
        let state = await states.next { if case .unreachable = $0 { true } else { false } }
        #expect(Self.wait(state) == 1)
        await link.stop()
    }

    @Test func anotherMajorWaitsThirtySeconds() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let mismatch = ErrorBody(
            code: "protocol.major_mismatch", category: .unsupported, message: "another major",
            retryable: false, resource: nil, fields: nil, hint: nil,
            details: [
                ErrorDetail(key: "serverMajor", value: "2"),
                ErrorDetail(key: "serverMinor", value: "3"),
            ],
            operationId: nil, rule: nil, items: nil)
        server.serve { session in
            let request = try session.readRequest()
            try session.fail(request.requestId, with: mismatch)
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        let state = await states.next { if case .incompatible = $0 { true } else { false } }
        #expect(
            state
                == .incompatible(
                    .majorMismatch(server: ProtocolVersion(major: 2, minor: 3), error: mismatch),
                    since: Self.now, nextAttempt: Self.now.addingTimeInterval(30)))
        await link.stop()
    }

    @Test func anUnansweredHandshakeIsUnreachableWithACallTimeout() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            _ = try session.readRequest()
            session.drainUntilClosed()
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        await clock.waitForSleepers(1)
        clock.advance(by: ContractLimits.handshakeTimeout)
        let state = await states.next { if case .unreachable = $0 { true } else { false } }
        let timeout = ClientErrorCatalog.callTimeout(
            method: "daemon.handshake", limit: .seconds(5))
        #expect(
            state
                == .unreachable(
                    timeout, since: Self.now, nextAttempt: Self.now.addingTimeInterval(0.5)))
        await link.stop()
    }

    @Test func anUntrustedSocketWaitsThirtySeconds() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath, mode: 0o666)
        defer { withExtendedLifetime(server) {} }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        let state = await states.next { if case .untrusted = $0 { true } else { false } }
        #expect(Self.wait(state) == 30)
        await link.stop()
        #expect(server.bytesReceived == 0)
    }

    @Test func aCompletedHandshakeResetsTheWaits() async throws {
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        _ = await states.next { Self.wait($0) == 0.5 }
        await clock.waitForSleepers(1)
        clock.advance(by: .milliseconds(500))
        _ = await states.next { Self.wait($0) == 1 }
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon()
            session.close()
        }
        await clock.waitForSleepers(1)
        clock.advance(by: .seconds(1))
        _ = await states.next { if case .connected = $0 { true } else { false } }
        let after = await states.next { if case .unreachable = $0 { true } else { false } }
        #expect(Self.wait(after) == 0.5)
        await link.stop()
    }

    @Test func theLinkHandshakesAsAnInteractiveGuiClient() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let sent = Mutex<ClientInfo?>(nil)
        server.serve { session in
            let request = try session.acceptHandshake()
            if case .daemonHandshake(let params) = request.call {
                sent.withLock { $0 = params.client }
            }
            try session.answerInfoAndHealth()
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        _ = await states.next { if case .connected = $0 { true } else { false } }
        let client = try #require(sent.withLock { $0 })
        #expect(client.kind == .gui)
        #expect(client.interactive == true)
        #expect(client.version == "0.1.0")
        #expect(client.pid == 42)
        await link.stop()
    }

    @Test func aMisconfiguredLinkNeverTries() async throws {
        let empty = makeLink(endpoint: .failure(.empty))
        var states = StateWatcher(empty)
        await empty.start()
        #expect(await states.next { _ in true } == .misconfigured(.rootOverride(.empty)))
        await empty.retryNow()
        #expect(empty.currentState == .misconfigured(.rootOverride(.empty)))

        let badVersion = makeLink(version: "0.1 beta")
        await badVersion.start()
        #expect(badVersion.currentState == .misconfigured(.invalidClientVersion))
        #expect(DaemonLink.isValidClientVersion("0.1.0"))
        #expect(DaemonLink.isValidClientVersion("") == false)
        #expect(DaemonLink.isValidClientVersion(String(repeating: "1", count: 65)) == false)
    }

    @Test func aRejectedCallNeverChangesTheStatus() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let unknown = Samples.error("request.unknown_method", .unsupported)
        server.serve { session in
            try session.startAsDaemon()
            let request = try session.readRequest()
            try session.fail(request.requestId, with: unknown)
            _ = try? session.readPayload()
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        let connected = await states.next { if case .connected = $0 { true } else { false } }
        await #expect(throws: ClientError.rejected(unknown)) {
            try await link.call(.daemonGetCapabilities)
        }
        #expect(link.currentState == connected)
        await link.stop()
    }

    @Test func aCallWhileNotConnectedIsNotSent() async throws {
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        _ = await states.next { if case .unreachable = $0 { true } else { false } }
        let error = await #expect(throws: ClientError.self) { try await link.call(.daemonGetInfo) }
        #expect(error?.body.code == "ipc.endpoint_missing")
        await link.stop()
    }

    @Test func aHeartbeatTimeoutMakesTheDaemonUnreachable() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon()
            _ = try session.readRequest()  // the heartbeat, never answered
            _ = try? session.readPayload()
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        _ = await states.next { if case .connected = $0 { true } else { false } }
        await clock.waitForSleepers(1)
        clock.advance(by: .seconds(15))
        await clock.waitForSleepers(1)
        clock.advance(by: .seconds(10))
        let state = await states.next { if case .unreachable = $0 { true } else { false } }
        guard case .unreachable(let error, _, _) = state else {
            Issue.record("expected unreachable")
            return
        }
        #expect(
            error
                == ClientErrorCatalog.callTimeout(method: "daemon.get_health", limit: .seconds(10)))
        await link.stop()
    }

    @Test func aStartingDaemonGetsAHeartbeatEverySecond() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon(health: .starting)
            let beat = try session.readRequest()
            try session.reply(to: beat, with: .daemonGetHealth(Samples.health(.ready)))
            _ = try? session.readPayload()
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        let first = await states.next { if case .connected = $0 { true } else { false } }
        guard case .connected(let starting) = first else { return }
        #expect(starting.health?.status == .starting)
        #expect(starting.info == Samples.info)
        await clock.waitForSleepers(1)
        clock.advance(by: .seconds(1))
        let next = await states.next {
            if case .connected(let daemon) = $0 { daemon.health?.status == .ready } else { false }
        }
        #expect(next != nil)
        await link.stop()
    }
}

/// Reads a link's states one by one.
struct StateWatcher {
    private var iterator: AsyncStream<ConnectionState>.Iterator
    private let link: DaemonLink

    init(_ link: DaemonLink) {
        self.link = link
        iterator = link.states.makeAsyncIterator()
    }

    /// The next state that matches `predicate`.
    mutating func next(_ predicate: (ConnectionState) -> Bool) async -> ConnectionState? {
        while let state = await iterator.next() {
            if predicate(state) { return state }
        }
        return nil
    }

    /// The link's current state.
    func latest() async -> ConnectionState? {
        link.currentState
    }
}
