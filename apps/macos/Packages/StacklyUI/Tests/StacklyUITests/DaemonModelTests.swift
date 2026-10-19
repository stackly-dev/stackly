import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization
import Testing

@testable import StacklyUI

/// The daemon model with a fake link: presence, banner, gate, page
/// placeholder, announcements, Diagnostics, and the mutation path (spec 0013
/// AC-9, AC-10, AC-11, AC-19).
@Suite(.timeLimit(.minutes(1)))
struct DaemonModelTests {
    static let launch = Date(timeIntervalSince1970: 1_792_245_720)

    /// A link the test drives.
    final class FakeLink: Sendable {
        let states: AsyncStream<ConnectionState>
        let push: AsyncStream<ConnectionState>.Continuation
        let retries = Mutex(0)
        let calls = Mutex<[(Call, String?)]>([])
        let result = Mutex<Result<Reply, ClientError>>(.success(.eventsUnsubscribe))

        init() {
            (states, push) = AsyncStream.makeStream()
        }

        var handle: LinkHandle {
            LinkHandle(
                states: { self.states },
                start: {},
                retryNow: { self.retries.withLock { $0 += 1 } },
                call: { call, key, _ throws(ClientError) in
                    self.calls.withLock { $0.append((call, key)) }
                    return try self.result.withLock { $0 }.get()
                },
                refresh: {},
                stopNow: {})
        }
    }

    /// One fixture: link, signals, model, announcements, and the one second
    /// timer the test fires.
    final class Fixture {
        let link = FakeLink()
        let signals: AsyncStream<SyncSignal>
        let signal: AsyncStream<SyncSignal>.Continuation
        var announced: [String] = []
        var clock = DaemonModelTests.launch
        let slowTimer: AsyncStream<Void>
        let fireSlowTimer: AsyncStream<Void>.Continuation
        var model: DaemonModel!

        init() {
            (signals, signal) = AsyncStream.makeStream()
            (slowTimer, fireSlowTimer) = AsyncStream.makeStream()
            let timer = slowTimer
            model = DaemonModel(
                link: link.handle,
                signals: signals,
                endpoint: SocketEndpoint(
                    runtimeRootBytes: Array("/t/stackly".utf8), rootOverride: false),
                launchedAt: DaemonModelTests.launch,
                now: { [unowned self] in clock },
                sleep: { _ in
                    for await _ in timer { return }
                },
                announce: { [unowned self] in announced.append($0) })
            model.start()
        }

        func push(_ state: ConnectionState) async throws {
            link.push.yield(state)
            try await waitUntil { self.model.connectionState == state }
        }

        func send(_ value: SyncSignal) async {
            signal.yield(value)
            for _ in 0..<20 { await Task.yield() }
        }
    }

    static func daemon(_ status: DaemonHealthStatus?, at seconds: TimeInterval = 10)
        -> ConnectedDaemon
    {
        let accepted = HandshakeAccepted(
            protocol: ProtocolVersion(major: 1, minor: 1),
            serverInstanceId: UUID(uuidString: "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e") ?? UUID(),
            daemon: DaemonIdentity(version: "0.1.0", pid: 501), contractDigest: ContractInfo.digest,
            contractDrift: false,
            limits: Limits(
                maxFrameBytes: 1_048_576, maxLogBatchBytes: 65_536, maxJsonDepth: 32,
                maxInFlightRequests: 32, maxSubscriptions: 8, eventBufferLen: 1_024,
                eventReplayLen: 4_096),
            head: EventCursor(
                serverInstanceId: UUID(uuidString: "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e")
                    ?? UUID(),
                sequence: 0))
        let health = status.map {
            DaemonHealth(
                status: $0,
                subsystems: [
                    SubsystemHealth(name: "registry", status: .degraded, detail: "slow disk")
                ],
                counters: DaemonCounters(
                    connectionsOpen: 1, connectionsTotal: 1, connectionsRejected: 0,
                    framesRejected: 0, eventsPublished: 0, eventsDropped: 0, logLinesDropped: 0,
                    socketRebinds: 0),
                recovery: nil)
        }
        let info = DaemonInfo(
            version: "0.1.0", protocol: ProtocolVersion(major: 1, minor: 1),
            contractDigest: ContractInfo.digest, serverInstanceId: accepted.serverInstanceId,
            pid: 501, uid: 501, startedAt: launch,
            roots: DaemonRoots(
                data: "/d", logs: "/l", cache: "/c", runtime: "/t/stackly", rootOverride: false),
            socketPath: "/t/stackly/stacklyd.sock", registrySchema: 2)
        return ConnectedDaemon(
            accepted: accepted, info: info, health: health,
            connectedAt: launch.addingTimeInterval(seconds))
    }

    static let missing = ClientErrorCatalog.endpointMissing(path: "/t/stackly/stacklyd.sock")

    static func unreachable(_ seconds: TimeInterval = 5) -> ConnectionState {
        .unreachable(
            missing, since: launch.addingTimeInterval(seconds),
            nextAttempt: launch.addingTimeInterval(seconds + 0.5))
    }

    // MARK: - Presence table

    @Test func presenceFollowsTheLinkAndTheRefresh() {
        typealias Inputs = DaemonPresence.Inputs
        #expect(DaemonPresence.derive(Inputs()) == .connecting(slow: false))
        #expect(DaemonPresence.derive(Inputs(stable: nil, slow: true)) == .connecting(slow: true))
        let ready = ConnectionState.connected(Self.daemon(.ready))
        #expect(DaemonPresence.derive(Inputs(stable: ready)) == .refreshing)
        #expect(
            DaemonPresence.derive(Inputs(stable: ready, refreshed: true))
                == .connected(health: .ready))
        #expect(
            DaemonPresence.derive(
                Inputs(stable: .connected(Self.daemon(.degraded)), refreshed: true))
                == .connected(health: .degraded))
        #expect(
            DaemonPresence.derive(
                Inputs(stable: .connected(Self.daemon(.unrecognized)), refreshed: true))
                == .connected(health: .unknown))
        #expect(
            DaemonPresence.derive(Inputs(stable: .connected(Self.daemon(nil)), refreshed: true))
                == .connected(health: .unknown))
        #expect(
            DaemonPresence.derive(
                Inputs(stable: .connected(Self.daemon(.starting)), refreshed: true))
                == .starting)
        #expect(
            DaemonPresence.derive(
                Inputs(stable: .connected(Self.daemon(.stopping)), refreshed: true))
                == .stopping)
        #expect(
            DaemonPresence.derive(Inputs(stable: ready, refreshed: true, recovery: true))
                == .recovery)
        let known = Self.launch.addingTimeInterval(60)
        #expect(
            DaemonPresence.derive(Inputs(stable: Self.unreachable(), lastKnownAt: known))
                == .unreachable(lastKnownAt: known))
        let untrusted = ClientErrorCatalog.endpointUntrusted(path: "/p", reason: .symlink)
        #expect(
            DaemonPresence.derive(
                Inputs(stable: .untrusted(untrusted, since: Self.launch, nextAttempt: Self.launch)))
                == .untrusted(reason: "symlink", path: "/p"))
        #expect(
            DaemonPresence.derive(Inputs(stable: .misconfigured(.invalidClientVersion)))
                == .misconfigured(.invalidClientVersion))
    }

    @Test func everyMisconfigurationMapsToItsShellWords() {
        let pairs: [(MisconfigurationReason, ShellStatus.Misconfiguration)] = [
            (.rootOverride(.empty), .rootEmpty), (.rootOverride(.notAbsolute), .rootNotAbsolute),
            (.rootOverride(.doesNotExist), .rootMissing),
            (.rootOverride(.unreadable(errno: 13)), .rootUnreadable),
            (.rootOverride(.runtimeRootTooLong(length: 80)), .rootTooLong),
            (.rootOverride(.tempDirUnavailable), .tempDirUnavailable),
            (.invalidClientVersion, .invalidClientVersion),
        ]
        for (reason, words) in pairs {
            #expect(ShellStatusMapping.misconfiguration(reason) == words)
        }
    }

    @Test func anotherMajorShowsBothProtocols() {
        let error = Samples.error("protocol.major_mismatch")
        let status = ShellStatusMapping.status(
            for: .incompatible(
                .majorMismatch(server: ProtocolVersion(major: 2, minor: 0), error: error)),
            stable: .incompatible(
                .majorMismatch(server: ProtocolVersion(major: 2, minor: 0), error: error),
                since: Self.launch, nextAttempt: Self.launch),
            launchedAt: Self.launch, before: .connecting(slow: false, since: Self.launch))
        #expect(
            status
                == .incompatible(
                    .majorMismatch(
                        daemonProtocol: "2.0", appProtocol: ShellStatusMapping.appProtocol),
                    since: Self.launch))
    }

    // MARK: - Without a daemon

    @Test(
        .evidence(
            covers: ["AC-024"],
            expected:
                "without stacklyd the shell keeps its window, says it cannot connect, blocks mutations, and shows no instance status"
        ))
    func withoutADaemonTheShellExplainsAndBlocksWithoutFlicker() async throws {
        let fixture = Fixture()
        let model = fixture.model!
        #expect(model.presence == .connecting(slow: false))
        #expect(model.presentation.banner == nil)
        #expect(model.presentation.pageFailure == nil)
        fixture.fireSlowTimer.yield()
        try await waitUntil { model.presence == .connecting(slow: true) }
        #expect(model.presentation.banner?.title == "Connecting to stacklyd…")

        try await fixture.push(Self.unreachable())
        #expect(model.presence == .unreachable(lastKnownAt: nil))
        #expect(model.presentation.footer.label == "stacklyd unavailable")
        let banner = try #require(model.presentation.banner)
        #expect(banner.title == "Can’t connect to stacklyd.")
        #expect(
            banner.message(
                now: Self.launch, calendar: .current, locale: .current, timeZone: .current)
                == "Actions are disabled until the daemon is reachable.")
        #expect(model.mutationAvailability.blockedReason == "stacklyd is unavailable.")
        #expect(model.presentation.pageFailure?.title == "Waiting for stacklyd")
        #expect(fixture.announced == ["Can’t connect to stacklyd."])

        try await fixture.push(.connecting(attempt: 2))
        #expect(model.presence == .unreachable(lastKnownAt: nil))
        #expect(model.presentation.isReconnecting)
        try await fixture.push(Self.unreachable())
        #expect(fixture.announced == ["Can’t connect to stacklyd."])
    }

    @Test func aMutationWhileNotConnectedIsNotSent() async throws {
        let fixture = Fixture()
        try await fixture.push(Self.unreachable())
        let outcome = await MutationPath(model: fixture.model).perform(.daemonGetInfo)
        #expect(outcome == .notSent(Self.missing))
        #expect(fixture.link.calls.withLock { $0.isEmpty })
        #expect(MutationFeedback.notSent == "stacklyd is unavailable. Nothing was sent.")
    }

    // MARK: - Losing and regaining the daemon

    @Test func aLostDaemonKeepsTheStateAndARecoveryWaitsForTheRefresh() async throws {
        let fixture = Fixture()
        let model = fixture.model!
        try await fixture.push(.connected(Self.daemon(.ready)))
        #expect(model.presence == .refreshing)
        #expect(model.mutationAvailability.blockedReason == "Loading the latest state.")
        fixture.clock = Self.launch.addingTimeInterval(20)
        await fixture.send(.resnapshot(.noCursor))
        try await waitUntil { model.presence == .connected(health: .ready) }
        #expect(model.mutationAvailability == .available)
        #expect(model.lastKnownAt == Self.launch.addingTimeInterval(20))
        await fixture.send(.lastFrameAt(Self.launch.addingTimeInterval(30)))
        #expect(model.lastKnownAt == Self.launch.addingTimeInterval(30))

        try await fixture.push(Self.unreachable(40))
        #expect(model.presence == .unreachable(lastKnownAt: Self.launch.addingTimeInterval(30)))
        let message = model.presentation.banner?.message(
            now: Self.launch.addingTimeInterval(41), calendar: .current, locale: .current,
            timeZone: .current)
        #expect(message?.hasPrefix("Showing the last known state from ") == true)
        #expect(model.mutationAvailability.blockedReason == "stacklyd is unavailable.")
        #expect(fixture.announced == ["Can’t connect to stacklyd."])

        try await fixture.push(.connected(Self.daemon(.ready, at: 50)))
        #expect(model.presence == .refreshing)
        #expect(model.presentation.banner?.title == "Can’t connect to stacklyd.")
        #expect(model.mutationAvailability.blockedReason == "stacklyd is unavailable.")
        await fixture.send(.caughtUp)
        try await waitUntil { model.presence == .connected(health: .ready) }
        #expect(model.presentation.banner == nil)
        #expect(
            fixture.announced == ["Can’t connect to stacklyd.", "Reconnected. State refreshed."])
    }

    @Test func aFailedManualReconnectIsAnnounced() async throws {
        let fixture = Fixture()
        try await fixture.push(Self.unreachable())
        fixture.model.reconnect()
        try await waitUntil { fixture.link.retries.withLock { $0 } == 1 }
        try await fixture.push(.connecting(attempt: 2))
        try await fixture.push(Self.unreachable(6))
        #expect(fixture.announced == ["Can’t connect to stacklyd.", "Still unreachable."])
    }

    @Test func connectedMutationsRunWithTheirKeyAndKeepItWhenTheOutcomeIsUnknown() async throws {
        let fixture = Fixture()
        try await fixture.push(.connected(Self.daemon(.ready)))
        let path = MutationPath(model: fixture.model)
        #expect(
            await path.perform(.daemonGetInfo, idempotencyKey: "k1")
                == .completed(.eventsUnsubscribe))
        #expect(fixture.link.calls.withLock { $0.last?.1 } == "k1")
        let lost = ClientErrorCatalog.connectionClosed()
        fixture.link.result.withLock { $0 = .failure(.outcomeUnknown(lost)) }
        #expect(
            await path.perform(.daemonGetInfo, idempotencyKey: "k2")
                == .outcomeUnknown(lost, key: "k2"))
        let refused = Samples.error("resource.busy")
        fixture.link.result.withLock { $0 = .failure(.rejected(refused)) }
        #expect(await path.perform(.daemonGetInfo) == .rejected(refused))
        let key = MutationPath.newKey()
        #expect(key.count == 36)
        #expect(key == key.lowercased())
        #expect(key != MutationPath.newKey())
    }

    @Test func healthAndRecoveryRowsFollowTheDaemon() async throws {
        let fixture = Fixture()
        let model = fixture.model!
        try await fixture.push(.connected(Self.daemon(.starting)))
        await fixture.send(.resnapshot(.noCursor))
        try await waitUntil { model.presence == .starting }
        #expect(model.presentation.banner?.title == "stacklyd is starting.")
        #expect(
            model.mutationAvailability
                == .blocked(
                    reason: "stacklyd is starting.", since: Self.launch.addingTimeInterval(10)))
        try await fixture.push(.connected(Self.daemon(.degraded)))
        #expect(model.presence == .connected(health: .degraded))
        #expect(model.presentation.footer.label == "stacklyd degraded")
        #expect(model.mutationAvailability == .available)
    }

    // MARK: - Diagnostics

    @Test func diagnosticsKeepTheLastKnownDaemonAfterALoss() async throws {
        let fixture = Fixture()
        let model = fixture.model!
        try await fixture.push(.connected(Self.daemon(.degraded)))
        await fixture.send(.resnapshot(.noCursor))
        try await fixture.push(Self.unreachable(40))
        let rows = model.diagnostics.rows(
            calendar: Calendar(identifier: .gregorian), locale: Locale(identifier: "en_US"),
            timeZone: .gmt)
        let values = Dictionary(uniqueKeysWithValues: rows.map { ($0.label, $0.value) })
        #expect(values["Status"] == "stacklyd unavailable")
        #expect(values["Socket"] == "/t/stackly/stacklyd.sock")
        #expect(values["Last error"]?.hasPrefix("ipc.endpoint_missing · unavailable") == true)
        #expect(values["stacklyd"]?.hasSuffix("(last known)") == true)
        #expect(values["Protocol"] == "stacklyd 1.1 · Stackly \(ShellStatusMapping.appProtocol)")
        #expect(values["Contract"] == "Matches")
        #expect(values["Health"] == "degraded · registry degraded: slow disk")
        #expect(values["Next attempt"] != nil)
        #expect(model.copyDiagnostics().contains("Status: stacklyd unavailable"))
    }

    @Test func anUnansweredHandshakeIsUnreachableWithTheFiveSecondHint() async throws {
        let fixture = Fixture()
        let model = fixture.model!
        let timeout = ClientErrorCatalog.callTimeout(method: "daemon.handshake", limit: .seconds(5))
        try await fixture.push(
            .unreachable(
                timeout, since: Self.launch.addingTimeInterval(5),
                nextAttempt: Self.launch.addingTimeInterval(5.5)))
        #expect(model.presence == .unreachable(lastKnownAt: nil))
        #expect(model.diagnostics.lastError?.method == "daemon.handshake")
        let rows = model.diagnostics.rows(
            calendar: Calendar(identifier: .gregorian), locale: Locale(identifier: "en_US"),
            timeZone: .gmt)
        let values = Dictionary(uniqueKeysWithValues: rows.map { ($0.label, $0.value) })
        #expect(values["What to do"] == "stacklyd didn’t answer within 5 seconds.")
    }

    @Test func aMisconfiguredAppNeverRetries() async throws {
        let fixture = Fixture()
        try await fixture.push(.misconfigured(.rootOverride(.empty)))
        #expect(fixture.model.presence == .misconfigured(.rootOverride(.empty)))
        #expect(fixture.model.presentation.isMisconfigured)
        fixture.model.reconnect()
        for _ in 0..<20 { await Task.yield() }
        #expect(fixture.link.retries.withLock { $0 } == 0)
        #expect(fixture.model.mutationAvailability.blockedReason == "STACKLY_ROOT is invalid.")
    }

    // MARK: - A real link to an empty socket path

    @Test func aRealLinkToAnEmptyPathEndsUnreachableWithoutAStateTime() async throws {
        let folder = "/private/tmp/stk-ui-\(UUID().uuidString.prefix(8).lowercased())"
        try FileManager.default.createDirectory(
            atPath: folder, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(atPath: folder) }
        let endpoint = SocketEndpoint(runtimeRootBytes: Array(folder.utf8), rootOverride: true)
        let link = DaemonLink(endpoint: .success(endpoint), clientVersion: "0.1.0", pid: 1)
        let model = DaemonModel(
            link: .live(link), signals: nil, endpoint: endpoint, launchedAt: Self.launch,
            announce: { _ in })
        model.start()
        try await waitUntil { model.presence == .unreachable(lastKnownAt: nil) }
        #expect(model.lastError?.code == "ipc.endpoint_missing")
        #expect(model.presentation.pageFailure?.code == "ipc.endpoint_missing")
        await link.stop()
    }
}

/// Sample values for the model tests.
enum Samples {
    static func error(_ code: String) -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: "m", retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
    }
}
