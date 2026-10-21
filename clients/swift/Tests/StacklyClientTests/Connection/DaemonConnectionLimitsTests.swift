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

    @Test func aCallAfterTheDaemonSentAConnectionErrorIsNotSentWithThatError() async throws {
        let shutdown = Samples.error("daemon.shutting_down", .unavailable)
        server.serve { session in
            try session.acceptHandshake()
            // Later than the handshake reply, so the connection is open when it arrives.
            Thread.sleep(forTimeInterval: 0.2)
            try session.fail(nil, with: shutdown)
            session.drainUntilClosed()
        }
        let (connection, _) = try await open()
        var reasons = connection.termination.makeAsyncIterator()
        #expect(await reasons.next() == shutdown)
        await #expect(throws: ClientError.notSent(shutdown)) {
            try await connection.call(Samples.unsubscribe(1))
        }
    }
}
