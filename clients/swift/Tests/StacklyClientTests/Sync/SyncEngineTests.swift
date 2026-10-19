import Foundation
import StacklyClient
import Synchronization
import Testing

/// The snapshot plus cursor recipe against a fake daemon (spec 0013 AC-8).
@Suite(.timeLimit(.minutes(1)))
struct SyncEngineTests {
    let root: ShortTempRoot
    let clock = ManualClock()
    let time = FakeNow()

    init() throws {
        root = try ShortTempRoot()
    }

    func makeEngine() -> (DaemonLink, SyncEngine) {
        let time = self.time
        let link = DaemonLink(
            endpoint: .success(root.endpoint), clientVersion: "0.1.0", pid: 42, clock: clock,
            now: { time.date })
        return (link, SyncEngine(link: link, now: { time.date }))
    }

    /// Reads the subscribe request and answers it for subscription 7 at `head`.
    static func acceptSubscribe(
        _ session: FakeSession, head: UInt64, instance: UUID = Samples.serverInstanceId
    )
        throws -> SubscribeParams?
    {
        let request = try session.readRequest()
        guard case .eventsSubscribe(let params) = request.call else {
            throw FakeServerError.unexpected("expected subscribe, got \(request.call.methodName)")
        }
        try session.reply(
            to: request,
            with: .eventsSubscribe(
                Subscribed(
                    subscriptionId: 7, head: EventCursor(serverInstanceId: instance, sequence: head)
                )))
        return params
    }

    @Test func withoutACursorItSnapshotsAndFollowsFromTheHead() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let params = Mutex<SubscribeParams?>(nil)
        server.serve { session in
            try session.startAsDaemon()
            let sent = try Self.acceptSubscribe(session, head: 5)
            params.withLock { $0 = sent }
            try session.sendEvent(subscription: 7, sequence: 6, kind: "instance.future_kind")
            try session.sendEvent(subscription: 7, sequence: 7, kind: "instance.future_kind")
            try session.sendEvent(subscription: 7, sequence: 6, kind: "instance.future_kind")
            _ = try? session.readPayload()
        }
        let (link, engine) = makeEngine()
        var signals = engine.signals.makeAsyncIterator()
        await engine.start()
        await link.start()
        #expect(await signals.next() == .resnapshot(.noCursor))
        #expect(await signals.next() == .lastFrameAt(time.date))
        try await waitUntil { await engine.cursor?.sequence == 7 }
        #expect(params.withLock { $0 }?.since == nil)
        #expect(params.withLock { $0 }?.kinds == [])
        #expect(
            await engine.cursor
                == EventCursor(serverInstanceId: Samples.serverInstanceId, sequence: 7))
        await engine.stop()
        await link.stop()
    }

    @Test func gapsAskForASnapshotAndTheStreamGoesOn() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon()
            _ = try Self.acceptSubscribe(session, head: 2)
            try session.sendEvent(
                subscription: 7, sequence: 3, kind: "events.gap", content: #"{"reason":"overflow"}"#
            )
            try session.sendEvent(subscription: 7, sequence: 4, kind: "instance.future_kind")
            try session.sendEvent(
                subscription: 7, sequence: 9, kind: "events.gap",
                content: #"{"reason":"cursor_expired"}"#)
            _ = try? session.readPayload()
        }
        let (link, engine) = makeEngine()
        var signals = engine.signals.makeAsyncIterator()
        await engine.start()
        await link.start()
        var resnapshots: [ResnapshotReason] = []
        while resnapshots.count < 3, let signal = await signals.next() {
            if case .resnapshot(let reason) = signal { resnapshots.append(reason) }
        }
        #expect(resnapshots == [.noCursor, .gap(.overflow), .gap(.cursorExpired)])
        try await waitUntil { await engine.cursor?.sequence == 9 }
        await engine.stop()
        await link.stop()
    }

    @Test func aReconnectResumesFromTheCursorAndARestartResetsIt() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let second = Mutex<SubscribeParams?>(nil)
        let restarted = UUID()
        server.serve { session in
            try session.startAsDaemon()
            _ = try Self.acceptSubscribe(session, head: 4)
            try session.sendEvent(subscription: 7, sequence: 5, kind: "instance.future_kind")
            Thread.sleep(forTimeInterval: 0.2)
            session.close()
        }
        let (link, engine) = makeEngine()
        var signals = engine.signals.makeAsyncIterator()
        var states = StateWatcher(link)
        await engine.start()
        await link.start()
        #expect(await signals.next() == .resnapshot(.noCursor))
        try await waitUntil { await engine.cursor?.sequence == 5 }
        _ = await states.next { if case .unreachable = $0 { true } else { false } }
        server.serve { session in
            try session.startAsDaemon(serverInstanceId: restarted)
            let params = try Self.acceptSubscribe(session, head: 2, instance: restarted)
            second.withLock { $0 = params }
            try session.sendEvent(
                subscription: 7, sequence: 2, kind: "events.gap",
                content: #"{"reason":"server_restarted"}"#)
            _ = try? session.readPayload()
        }
        await clock.waitForSleepers(1)
        clock.advance(by: .milliseconds(500))
        var reason: ResnapshotReason?
        while reason == nil, let signal = await signals.next() {
            if case .resnapshot(let value) = signal { reason = value }
        }
        #expect(reason == .gap(.serverRestarted))
        #expect(
            second.withLock { $0 }?.since
                == EventCursor(serverInstanceId: Samples.serverInstanceId, sequence: 5))
        try await waitUntil {
            await engine.cursor == EventCursor(serverInstanceId: restarted, sequence: 2)
        }
        await engine.stop()
        await link.stop()
    }

    @Test func aReconnectWithAValidCursorCatchesUpWithoutASnapshot() async throws {
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
            _ = try Self.acceptSubscribe(session, head: 5)
            try session.sendEvent(subscription: 7, sequence: 4, kind: "instance.x")
            try session.sendEvent(subscription: 7, sequence: 5, kind: "instance.x")
            _ = try? session.readPayload()
        }
        await clock.waitForSleepers(1)
        clock.advance(by: .milliseconds(500))
        var rest: [SyncSignal] = []
        while rest.contains(.caughtUp) == false, let signal = await signals.next() {
            if case .lastFrameAt = signal { continue }
            rest.append(signal)
        }
        #expect(rest == [.caughtUp])
        #expect(await engine.cursor?.sequence == 5)
        await engine.stop()
        await link.stop()
    }

    @Test func shuttingDownMakesTheLinkUnreachableAtOnce() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon()
            _ = try Self.acceptSubscribe(session, head: 1)
            try session.sendEvent(
                subscription: 7, sequence: 2, kind: "daemon.shutting_down",
                content: #"{"reason":"signal"}"#)
            _ = try? session.readPayload()
        }
        let (link, engine) = makeEngine()
        var states = StateWatcher(link)
        await engine.start()
        await link.start()
        let state = await states.next { if case .unreachable = $0 { true } else { false } }
        guard case .unreachable(let error, _, _) = state else { return }
        #expect(error.code == "daemon.shutting_down")
        await engine.stop()
        await link.stop()
    }

    @Test func frameTimesAreSentAtMostOncePerSecond() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let time = self.time
        server.serve { session in
            try session.startAsDaemon()
            _ = try Self.acceptSubscribe(session, head: 1)
            for sequence in 2...4 {
                try session.sendEvent(
                    subscription: 7, sequence: UInt64(sequence), kind: "instance.x")
            }
            Thread.sleep(forTimeInterval: 0.2)
            time.advance(by: 2)
            try session.sendEvent(subscription: 7, sequence: 5, kind: "instance.x")
            _ = try? session.readPayload()
        }
        let (link, engine) = makeEngine()
        var signals = engine.signals.makeAsyncIterator()
        await engine.start()
        await link.start()
        var frameTimes: [Date] = []
        while frameTimes.count < 2, let signal = await signals.next() {
            if case .lastFrameAt(let date) = signal { frameTimes.append(date) }
        }
        try await waitUntil { await engine.cursor?.sequence == 5 }
        #expect(frameTimes.count == 2)
        #expect(frameTimes.last?.timeIntervalSince(frameTimes[0]) == 2)
        await engine.stop()
        await link.stop()
    }
}

/// Waits until `condition` holds, checking every few milliseconds.
func waitUntil(_ condition: () async -> Bool) async throws {
    for _ in 0..<2000 {
        if await condition() { return }
        try await Task.sleep(for: .milliseconds(5))
    }
    Issue.record("the condition never held")
}
