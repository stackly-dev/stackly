import Foundation
import StacklyClient
import Synchronization
import Testing

/// Edges of the connection's request bookkeeping: request IDs, the in flight
/// limit, the abandoned set, cancel after a write, and the error a closed
/// connection keeps for later calls (spec 0013 AC-6, AC-19).
extension DaemonConnectionTests {
    @Test func everyCallGetsItsOwnLowercaseRequestId() async throws {
        let ids = Mutex<[String]>([])
        server.serve { session in
            try session.acceptHandshake()
            for _ in 0..<3 {
                let request = try session.readRequest()
                ids.withLock { $0.append(request.requestId) }
                try session.reply(to: request, with: .eventsUnsubscribe)
            }
        }
        let (connection, _) = try await open()
        for id in 1...3 {
            #expect(
                try await connection.call(Samples.unsubscribe(UInt32(id))) == .eventsUnsubscribe)
        }
        let seen = ids.withLock { $0 }
        #expect(seen.count == 3)
        #expect(Set(seen).count == 3)
        for id in seen {
            #expect(id == id.lowercased())
            #expect(UUID(uuidString: id) != nil)
        }
        await connection.close()
    }

    @Test func aCallLimitOfTenSecondsIsTheDefault() {
        #expect(CallOptions.standard.timeout == .seconds(10))
        #expect(ContractLimits.callTimeout == .seconds(10))
    }

    @Test func anInFlightLimitOfZeroStillLetsOneCallThrough() async throws {
        server.serve { session in
            try session.acceptHandshake(limits: Samples.limits.withInFlight(0))
            let first = try session.readRequest()
            try session.reply(to: first, with: .eventsUnsubscribe)
            let second = try session.readRequest()
            try session.reply(to: second, with: .eventsUnsubscribe)
        }
        let (connection, _) = try await open()
        #expect(try await connection.call(Samples.unsubscribe(1)) == .eventsUnsubscribe)
        #expect(try await connection.call(Samples.unsubscribe(2)) == .eventsUnsubscribe)
        await connection.close()
    }

    @Test func aCallCancelledAfterItsFrameWasWrittenIsOutcomeUnknownWithItsKey() async throws {
        let read = Mutex(false)
        server.serve { session in
            try session.acceptHandshake()
            _ = try session.readRequest()
            read.withLock { $0 = true }
            session.drainUntilClosed()
        }
        let (connection, _) = try await open()
        let call = Task {
            try await connection.call(Samples.unsubscribe(1), idempotencyKey: "key-1")
        }
        try await waitUntil { read.withLock { $0 } }
        call.cancel()
        let result = await call.result
        let timeout = ClientErrorCatalog.callTimeout(
            method: "events.unsubscribe", limit: ContractLimits.callTimeout,
            idempotencyKey: "key-1")
        #expect(timeout.detail("idempotencyKey") == "key-1")
        #expect(throws: ClientError.outcomeUnknown(timeout)) { try result.get() }
        await connection.close()
    }

    @Test func aLateReplyIsDroppedOnlyWhileItsCallIsAmongTheLastAbandonedOnes() async throws {
        let clock = ManualClock()
        let allTimedOut = Mutex(false)
        server.serve { session in
            // One in flight, so the abandoned set holds at most two request IDs.
            try session.acceptHandshake(limits: Samples.limits.withInFlight(1))
            var requests: [Request] = []
            for _ in 0..<3 {
                requests.append(try session.readRequest())
            }
            while allTimedOut.withLock({ $0 }) == false {
                Thread.sleep(forTimeInterval: 0.01)
            }
            // The third and second call are still remembered: quiet.
            try session.reply(to: requests[2], with: .eventsUnsubscribe)
            try session.reply(to: requests[1], with: .eventsUnsubscribe)
            let fourth = try session.readRequest()
            try session.reply(to: fourth, with: .eventsUnsubscribe)
            // The first was pushed out of the set: a protocol violation.
            try session.reply(to: requests[0], with: .eventsUnsubscribe)
            session.drainUntilClosed()
        }
        let (connection, _) = try await open()
        let options = CallOptions(timeout: .seconds(10), clock: clock)
        for id in 1...3 {
            let call = Task {
                try await connection.call(Samples.unsubscribe(UInt32(id)), options: options)
            }
            await clock.waitForSleepers(1)
            clock.advance(by: .seconds(10))
            let result = await call.result
            #expect(throws: ClientError.self) { try result.get() }
        }
        allTimedOut.withLock { $0 = true }
        #expect(try await connection.call(Samples.unsubscribe(4)) == .eventsUnsubscribe)
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next()?.code == "ipc.protocol_violation")
    }

    @Test func aHeaderReadWithTheHandshakeReplyIsCheckedAgainstASmallerDaemonLimit() async throws {
        server.serve { session in
            let request = try session.readRequest()
            // The next frame's header in the same write as the reply, so the
            // client reads both in one chunk, before it knows the limit.
            try session.sendRaw(
                Self.handshakeReply(to: request, limits: Samples.limits.withFrameBytes(64))
                    + [0x00, 0x00, 0x01, 0x00])
            session.close()
        }
        let (connection, _) = try await open()
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next() == ClientErrorCatalog.frame(.tooLarge(length: 256, max: 64)))
    }

    @Test func aFrameReadWithTheHandshakeReplyMayUseALargerDaemonLimit() async throws {
        let limit: UInt32 = 2 * ContractLimits.controlFrameMax
        let padding = String(repeating: "x", count: Int(ContractLimits.controlFrameMax))
        let event = Array(
            #"{"type":"event","content":{"subscriptionId":99,"sequence":1,"at":"2026-10-18T03:04:05.123Z","kind":{"type":"instance.future_kind","content":{"pad":"\#(padding)"}}}}"#
                .utf8)
        server.serve { session in
            let request = try session.readRequest()
            // The reply and the header of a frame above 1 MiB, but within the
            // daemon's limit, in one write; the payload follows.
            let frame = try FrameEncoder.encode(event, maxLength: UInt32.max)
            try session.sendRaw(
                Self.handshakeReply(to: request, limits: Samples.limits.withFrameBytes(limit))
                    + frame.prefix(FrameDecoder.headerLength))
            try session.sendRaw(Array(frame.dropFirst(FrameDecoder.headerLength)))
            let call = try session.readRequest()
            try session.reply(to: call, with: .eventsUnsubscribe)
        }
        let (connection, _) = try await open()
        #expect(event.count > Int(ContractLimits.controlFrameMax))
        #expect(try await connection.call(Samples.unsubscribe(1)) == .eventsUnsubscribe)
        await connection.close()
    }

    /// The framed handshake reply to `request` with `limits`.
    static func handshakeReply(to request: Request, limits: Limits) throws -> [UInt8] {
        let reply = ServerFrame.response(
            Response(
                requestId: request.requestId,
                reply: .daemonHandshake(Samples.accepted(limits: limits))))
        return try FrameEncoder.encode(
            Array(StacklyJSON.encoder().encode(reply)), maxLength: UInt32.max)
    }

    @Test func aCallAfterTheDaemonSentAConnectionErrorIsNotSentWithThatError() async throws {
        let shutdown = Samples.error("daemon.shutting_down", .unavailable)
        server.serve { session in
            let request = try session.readRequest()
            // In the same read as the handshake reply, so it is handled before
            // `open` gets the reply back, the way an EOF right behind the reply
            // can be. The connection is open from the moment the reply is read.
            try session.sendTogether([
                .response(
                    Response(
                        requestId: request.requestId, reply: .daemonHandshake(Samples.accepted()))),
                .error(ErrorFrame(requestId: nil, error: shutdown)),
            ])
            session.drainUntilClosed()
        }
        let (connection, accepted) = try await open()
        #expect(accepted == Samples.accepted())
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next() == shutdown)
        await #expect(throws: ClientError.notSent(shutdown)) {
            try await connection.call(Samples.unsubscribe(1))
        }
    }
}
