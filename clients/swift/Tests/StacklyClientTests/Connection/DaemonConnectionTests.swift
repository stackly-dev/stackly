import Foundation
import StacklyClient
import Synchronization
import Testing

/// The connection against a scripted fake daemon on a real socket: handshake,
/// calls, time outs, cancels, and how a closing connection fails its calls
/// (spec 0013 AC-5, AC-6, AC-19).
@Suite(.timeLimit(.minutes(1)))
struct DaemonConnectionTests {
    let root: ShortTempRoot
    let server: FakeDaemonServer

    init() throws {
        root = try ShortTempRoot()
        server = try FakeDaemonServer(path: root.endpoint.socketPath)
    }

    func open(options: CallOptions = CallOptions(timeout: .seconds(5))) async throws
        -> (DaemonConnection, HandshakeAccepted)
    {
        try await DaemonConnection.open(
            endpoint: root.endpoint, policy: .sameUser, client: Samples.client, options: options)
    }

    @Test func handshakeSendsTheContractAndReturnsTheLimits() async throws {
        let sent = Mutex<Request?>(nil)
        server.serve { session in
            let request = try session.acceptHandshake()
            sent.withLock { $0 = request }
        }
        let (connection, accepted) = try await open()
        #expect(accepted == Samples.accepted())
        let request = try #require(sent.withLock { $0 })
        guard case .daemonHandshake(let params) = request.call else {
            Issue.record("first frame was not the handshake")
            return
        }
        #expect(params.protocol == ContractInfo.protocolVersion)
        #expect(params.contractDigest == ContractInfo.digest)
        #expect(params.client == Samples.client)
        #expect(request.requestId == request.requestId.lowercased())
        #expect(UUID(uuidString: request.requestId) != nil)
        await connection.close()
    }

    @Test func anotherMajorIsRejectedWithTheDaemonsError() async throws {
        let mismatch = Samples.error("protocol.major_mismatch", .unsupported)
        server.serve { session in
            let request = try session.readRequest()
            try session.fail(request.requestId, with: mismatch)
        }
        await #expect(throws: ClientError.rejected(mismatch)) { try await open() }
    }

    @Test func aRejectedClientVersionIsRejected() async throws {
        let invalid = Samples.error("request.invalid_params", .invalidInput)
        server.serve { session in
            let request = try session.readRequest()
            try session.fail(request.requestId, with: invalid)
        }
        await #expect(throws: ClientError.rejected(invalid)) { try await open() }
    }

    @Test func contractDriftKeepsTheConnection() async throws {
        server.serve { session in
            try session.acceptHandshake(contractDrift: true)
            let request = try session.readRequest()
            try session.reply(to: request, with: .eventsUnsubscribe)
        }
        let (connection, accepted) = try await open()
        #expect(accepted.contractDrift)
        #expect(try await connection.call(Samples.unsubscribe(1)) == .eventsUnsubscribe)
        await connection.close()
    }

    @Test func nothingButTheHandshakeIsWrittenBeforeItIsAccepted() async throws {
        let extraBytes = Mutex(-1)
        server.serve { session in
            _ = try session.readRequest()
            // Wait a moment: a client that wrote a second frame would show it.
            Thread.sleep(forTimeInterval: 0.3)
            extraBytes.withLock { $0 = session.pendingByteCount() }
            session.close()
        }
        await #expect(throws: ClientError.self) { try await open() }
        #expect(extraBytes.withLock { $0 } == 0)
    }

    @Test func repliesAreMatchedByRequestIdInAnyOrder() async throws {
        server.serve { session in
            try session.acceptHandshake()
            let first = try session.readRequest()
            let second = try session.readRequest()
            try session.reply(to: second, with: .eventsUnsubscribe)
            try session.reply(to: first, with: .eventsUnsubscribe)
        }
        let (connection, _) = try await open()
        async let one = connection.call(Samples.unsubscribe(1))
        async let two = connection.call(Samples.unsubscribe(2))
        #expect(try await one == .eventsUnsubscribe)
        #expect(try await two == .eventsUnsubscribe)
        await connection.close()
    }

    @Test func callsAboveTheInFlightLimitWaitForASlot() async throws {
        let seen = Mutex(0)
        server.serve { session in
            try session.acceptHandshake(limits: Samples.limits.withInFlight(2))
            let first = try session.readRequest()
            _ = try session.readRequest()
            // The third call must wait: nothing more arrives until one ends.
            seen.withLock { $0 = session.pendingByteCount() }
            try session.reply(to: first, with: .eventsUnsubscribe)
            let third = try session.readRequest()
            try session.reply(to: third, with: .eventsUnsubscribe)
        }
        let (connection, _) = try await open()
        async let one = connection.call(Samples.unsubscribe(1))
        let two = Task { try await connection.call(Samples.unsubscribe(2)) }
        try await Task.sleep(for: .milliseconds(100))
        async let three = connection.call(Samples.unsubscribe(3))
        #expect(try await one == .eventsUnsubscribe)
        #expect(try await three == .eventsUnsubscribe)
        #expect(seen.withLock { $0 } == 0)
        two.cancel()
        await connection.close()
    }

    @Test func aTimeoutFailsOnlyThatCallAsOutcomeUnknown() async throws {
        let clock = ManualClock()
        server.serve { session in
            try session.acceptHandshake()
            let slow = try session.readRequest()
            let fast = try session.readRequest()
            try session.reply(to: fast, with: .eventsUnsubscribe)
            // A late reply for the abandoned call is dropped quietly.
            Thread.sleep(forTimeInterval: 0.2)
            try session.reply(to: slow, with: .eventsUnsubscribe)
            let after = try session.readRequest()
            try session.reply(to: after, with: .eventsUnsubscribe)
        }
        let (connection, _) = try await open()
        let options = CallOptions(timeout: .seconds(10), clock: clock)
        let slow = Task { try await connection.call(Samples.unsubscribe(1), options: options) }
        await clock.waitForSleepers(1)
        #expect(try await connection.call(Samples.unsubscribe(2)) == .eventsUnsubscribe)
        clock.advance(by: .seconds(10))
        let result = await slow.result
        let timeout = ClientErrorCatalog.callTimeout(method: "events.unsubscribe")
        #expect(throws: ClientError.outcomeUnknown(timeout)) { try result.get() }
        try await Task.sleep(for: .milliseconds(400))
        #expect(try await connection.call(Samples.unsubscribe(3)) == .eventsUnsubscribe)
        await connection.close()
    }

    @Test func aCallCancelledBeforeItHasASlotIsNotSent() async throws {
        server.serve { session in
            try session.acceptHandshake(limits: Samples.limits.withInFlight(1))
            let first = try session.readRequest()
            Thread.sleep(forTimeInterval: 0.3)
            try session.reply(to: first, with: .eventsUnsubscribe)
        }
        let (connection, _) = try await open()
        let first = Task { try await connection.call(Samples.unsubscribe(1)) }
        try await Task.sleep(for: .milliseconds(50))
        let second = Task { try await connection.call(Samples.unsubscribe(2)) }
        try await Task.sleep(for: .milliseconds(50))
        second.cancel()
        let result = await second.result
        #expect(
            throws: ClientError.notSent(
                ClientErrorCatalog.callTimeout(method: "events.unsubscribe"))
        ) {
            try result.get()
        }
        #expect(try await first.value == .eventsUnsubscribe)
        #expect(server.bytesReceived > 0)
        await connection.close()
    }

    @Test func anErrorWithoutRequestIdAfterTheHandshakeFailsWaitingCalls() async throws {
        let shutdown = Samples.error("daemon.shutting_down", .unavailable)
        server.serve { session in
            try session.acceptHandshake()
            _ = try session.readRequest()
            try session.fail(nil, with: shutdown)
        }
        let (connection, _) = try await open()
        await #expect(throws: ClientError.outcomeUnknown(shutdown)) {
            try await connection.call(Samples.unsubscribe(1))
        }
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next() == shutdown)
    }

    @Test func aDaemonErrorForTheCallIsRejected() async throws {
        let busy = Samples.error("resource.busy", .conflict)
        server.serve { session in
            try session.acceptHandshake()
            let request = try session.readRequest()
            try session.fail(request.requestId, with: busy)
        }
        let (connection, _) = try await open()
        await #expect(throws: ClientError.rejected(busy)) {
            try await connection.call(Samples.unsubscribe(1))
        }
        await connection.close()
    }

    @Test func aReplyWithAnotherTagIsAProtocolViolation() async throws {
        server.serve { session in
            try session.acceptHandshake()
            let request = try session.readRequest()
            try session.reply(to: request, with: .eventsUnsubscribe)
        }
        let (connection, _) = try await open()
        await #expect(throws: ClientError.outcomeUnknown(ClientErrorCatalog.connectionClosed())) {
            try await connection.call(.daemonGetInfo)
        }
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next()?.code == "ipc.protocol_violation")
    }

    @Test func aReplyForAnUnknownRequestIdIsAProtocolViolation() async throws {
        server.serve { session in
            try session.acceptHandshake()
            Thread.sleep(forTimeInterval: 0.2)
            try session.send(.response(Response(requestId: "not-asked", reply: .eventsUnsubscribe)))
        }
        let (connection, _) = try await open()
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next()?.code == "ipc.protocol_violation")
    }

    @Test func aCallAfterTheConnectionClosedIsNotSent() async throws {
        server.serve { session in
            try session.acceptHandshake()
            session.close()
        }
        let (connection, _) = try await open()
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next() == ClientErrorCatalog.connectionClosed())
        await #expect(throws: ClientError.notSent(ClientErrorCatalog.connectionClosed())) {
            try await connection.call(Samples.unsubscribe(1))
        }
    }

    @Test func aFrameAboveTheLimitClosesTheConnection() async throws {
        server.serve { session in
            try session.acceptHandshake(limits: Samples.limits.withFrameBytes(64))
            // Later than the handshake reply, so the daemon's limit applies.
            Thread.sleep(forTimeInterval: 0.2)
            try session.sendRaw([0x00, 0x00, 0x01, 0x00])
        }
        let (connection, _) = try await open()
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next()?.code == "ipc.frame_too_large")
    }

    @Test func subscribeDeliversEventsAndSkippedKindsInOrder() async throws {
        server.serve { session in
            try session.acceptHandshake()
            let request = try session.readRequest()
            let head = EventCursor(serverInstanceId: Samples.serverInstanceId, sequence: 0)
            try session.reply(
                to: request, with: .eventsSubscribe(Subscribed(subscriptionId: 7, head: head)))
            try session.sendPayload(
                Array(
                    #"{"type":"event","content":{"subscriptionId":7,"sequence":1,"at":"2026-10-18T03:04:05.123Z","kind":{"type":"instance.future_kind","content":{"x":1}}}}"#
                        .utf8))
            try session.send(
                .event(
                    Event(
                        subscriptionId: 7, sequence: 2, at: Date(timeIntervalSince1970: 0),
                        resource: nil, revision: nil, kind: .eventsGap(Gap(reason: .overflow)))))
            session.close()
        }
        let (connection, _) = try await open()
        let (subscribed, stream) = try await connection.subscribe(
            SubscribeParams(since: nil, kinds: [], resource: nil))
        #expect(subscribed.subscriptionId == 7)
        var frames: [DecodedFrame] = []
        do {
            for try await frame in stream { frames.append(frame) }
        } catch {}
        #expect(frames.count == 2)
        #expect(
            frames.first == .skipped(subscriptionId: 7, sequence: 1, kind: "instance.future_kind"))
        if case .event(let event) = frames.last {
            #expect(event.sequence == 2)
        } else {
            Issue.record("the second frame is not an event")
        }
    }
}

extension FakeSession {
    /// Bytes the client sent that the script has not read yet, without
    /// waiting.
    func pendingByteCount() -> Int {
        peekAvailable()
    }
}

extension Limits {
    /// These limits with another in flight limit.
    func withInFlight(_ value: UInt32) -> Limits {
        Limits(
            maxFrameBytes: maxFrameBytes, maxLogBatchBytes: maxLogBatchBytes,
            maxJsonDepth: maxJsonDepth, maxInFlightRequests: value,
            maxSubscriptions: maxSubscriptions, eventBufferLen: eventBufferLen,
            eventReplayLen: eventReplayLen)
    }

    /// These limits with another frame limit.
    func withFrameBytes(_ value: UInt32) -> Limits {
        Limits(
            maxFrameBytes: value, maxLogBatchBytes: maxLogBatchBytes,
            maxJsonDepth: maxJsonDepth, maxInFlightRequests: maxInFlightRequests,
            maxSubscriptions: maxSubscriptions, eventBufferLen: eventBufferLen,
            eventReplayLen: eventReplayLen)
    }
}
