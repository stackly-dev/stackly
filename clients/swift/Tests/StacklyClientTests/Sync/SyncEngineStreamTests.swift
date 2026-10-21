import Foundation
import StacklyClient
import Synchronization
import Testing

/// What reaches the stores from the event stream, and what a gap does while a
/// reconnect is still catching up (spec 0013 AC-8).
extension SyncEngineTests {
    @Test func knownEventsReachTheStoresInOrderAndOnesAlreadyAppliedAreDropped() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let removed = #"{"instanceId":"0192f3a5-0000-7000-8000-000000000002"}"#
        server.serve { session in
            try session.startAsDaemon()
            _ = try Self.acceptSubscribe(session, head: 5)
            for sequence: UInt64 in [6, 7, 6, 5, 8] {
                try session.sendEvent(
                    subscription: 7, sequence: sequence, kind: "instance.removed", content: removed)
            }
            _ = try? session.readPayload()
        }
        let (link, engine) = makeEngine()
        var signals = engine.signals.makeAsyncIterator()
        await engine.start()
        await link.start()
        var applied: [UInt64] = []
        while applied.last != 8, let signal = await signals.next() {
            if case .event(let event) = signal { applied.append(event.sequence) }
        }
        #expect(applied == [6, 7, 8])
        #expect(await engine.cursor?.sequence == 8)
        await engine.stop()
        await link.stop()
    }

    @Test func aGapWhileCatchingUpAsksForASnapshotInsteadOfSayingCaughtUp() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon()
            _ = try Self.acceptSubscribe(session, head: 3)
            Thread.sleep(forTimeInterval: 0.1)
            session.close()
        }
        let (link, engine) = makeEngine()
        var signals = engine.signals.makeAsyncIterator()
        var states = StateWatcher(link)
        await engine.start()
        await link.start()
        #expect(await signals.next() == .resnapshot(.noCursor))
        _ = await states.next { if case .unreachable = $0 { true } else { false } }
        server.serve { session in
            try session.startAsDaemon()
            _ = try Self.acceptSubscribe(session, head: 10)
            try session.sendEvent(
                subscription: 7, sequence: 10, kind: "events.gap",
                content: #"{"reason":"cursor_expired"}"#)
            _ = try? session.readPayload()
        }
        await clock.waitForSleepers(1)
        clock.advance(by: .milliseconds(500))
        var rest: [SyncSignal] = []
        while rest.contains(.resnapshot(.gap(.cursorExpired))) == false,
            let signal = await signals.next()
        {
            if case .lastFrameAt = signal { continue }
            rest.append(signal)
        }
        #expect(rest == [.resnapshot(.gap(.cursorExpired))])
        try await waitUntil { await engine.cursor?.sequence == 10 }
        await engine.stop()
        await link.stop()
    }
}
