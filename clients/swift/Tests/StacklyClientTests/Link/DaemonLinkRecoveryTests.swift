import Foundation
import StacklyClient
import Synchronization
import Testing

/// The link's remaining status rules: which failures block, what Reconnect
/// skips, how `since` behaves, and how the heartbeat treats answers (spec
/// 0013 AC-5, AC-7, AC-11).
extension DaemonLinkTests {
    @Test func aHandshakeTheDaemonRefusesAsInvalidIsIncompatibleAndWaitsThirtySeconds() async throws
    {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let invalid = Samples.error("request.invalid_params", .invalidInput)
        server.serve { session in
            let request = try session.readRequest()
            try session.fail(request.requestId, with: invalid)
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        let state = await states.next { if case .incompatible = $0 { true } else { false } }
        #expect(
            state
                == .incompatible(
                    .handshakeRejected(invalid), since: Self.now,
                    nextAttempt: Self.now.addingTimeInterval(30)))
        await link.stop()
    }

    @Test func aStaleSocketIsUnreachableWithConnectionRefused() async throws {
        try UnixSocketTests.makeStaleSocket(at: root.endpoint.socketPath)
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        let state = await states.next { if case .unreachable = $0 { true } else { false } }
        guard case .unreachable(let error, _, _) = state else {
            Issue.record("expected unreachable")
            return
        }
        #expect(error.code == "ipc.connection_refused")
        #expect(Self.wait(state) == 0.5)
        await link.stop()
    }

    @Test func reconnectDuringAnUntrustedWaitTriesAtOnceWithoutMovingTheClock() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath, mode: 0o666)
        defer { withExtendedLifetime(server) {} }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        _ = await states.next { if case .untrusted = $0 { true } else { false } }
        await clock.waitForSleepers(1)
        await link.retryNow()
        let second = await states.next {
            if case .connecting(let attempt) = $0 { attempt == 2 } else { false }
        }
        #expect(second != nil)
        let again = await states.next { if case .untrusted = $0 { true } else { false } }
        #expect(Self.wait(again) == 30)
        await link.stop()
        #expect(server.bytesReceived == 0)
    }

    @Test func sinceStaysWhileTheSameProblemRepeatsAndStartsOverWhenTheProblemChanges() async throws
    {
        let time = FakeNow()
        let start = time.date
        let link = DaemonLink(
            endpoint: .success(root.endpoint), clientVersion: "0.1.0", pid: 42, clock: clock,
            now: { time.date })
        var states = StateWatcher(link)
        await link.start()
        let first = await states.next { if case .unreachable = $0 { true } else { false } }
        await clock.waitForSleepers(1)
        time.advance(by: 0.5)
        clock.advance(by: .milliseconds(500))
        let second = await states.next { if case .unreachable = $0 { true } else { false } }
        guard case .unreachable(_, let firstSince, _) = first,
            case .unreachable(_, let secondSince, let secondNext) = second
        else {
            Issue.record("expected two unreachable states")
            return
        }
        #expect(firstSince == start)
        #expect(secondSince == start)
        #expect(secondNext == start.addingTimeInterval(0.5 + 1))

        let server = try FakeDaemonServer(path: root.endpoint.socketPath, mode: 0o666)
        defer { withExtendedLifetime(server) {} }
        await clock.waitForSleepers(1)
        time.advance(by: 1)
        clock.advance(by: .seconds(1))
        let untrusted = await states.next { if case .untrusted = $0 { true } else { false } }
        guard case .untrusted(_, let untrustedSince, _) = untrusted else {
            Issue.record("expected untrusted")
            return
        }
        #expect(untrustedSince == start.addingTimeInterval(1.5))
        await link.stop()
    }

    @Test func aReadyDaemonGetsAHeartbeatEveryFifteenSecondsNotSooner() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon()
            let beat = try session.readRequest()
            try session.reply(to: beat, with: .daemonGetHealth(Samples.health(.degraded)))
            _ = try? session.readPayload()
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        let first = await states.next { if case .connected = $0 { true } else { false } }
        guard case .connected(let ready) = first else { return }
        #expect(ready.health?.status == .ready)
        let before = server.bytesReceived
        await clock.waitForSleepers(1)
        clock.advance(by: .milliseconds(14_900))
        try await Task.sleep(for: .milliseconds(150))
        #expect(server.bytesReceived == before)
        clock.advance(by: .milliseconds(100))
        let next = await states.next {
            if case .connected(let daemon) = $0 {
                daemon.health?.status == .degraded
            } else {
                false
            }
        }
        #expect(next != nil)
        await link.stop()
    }

    @Test func aHeartbeatTheDaemonRejectsKeepsTheConnectionAndTheNextOneStillRuns() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        let rejected = Mutex(false)
        let unknown = Samples.error("request.unknown_method", .unsupported)
        server.serve { session in
            try session.startAsDaemon()
            let first = try session.readRequest()
            try session.fail(first.requestId, with: unknown)
            rejected.withLock { $0 = true }
            let second = try session.readRequest()
            try session.reply(to: second, with: .daemonGetHealth(Samples.health(.degraded)))
            _ = try? session.readPayload()
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        _ = await states.next { if case .connected = $0 { true } else { false } }
        await clock.waitForSleepers(1)
        clock.advance(by: .seconds(15))
        try await waitUntil { rejected.withLock { $0 } }
        // The link took the rejection and went back to sleep for the next
        // interval, not the call's 10 second limit.
        await clock.waitForSleeper(dueIn: .seconds(15))
        clock.advance(by: .seconds(15))
        var seen: [ConnectionState] = []
        while let state = await states.next({ _ in true }) {
            seen.append(state)
            if case .connected(let daemon) = state, daemon.health?.status == .degraded { break }
        }
        let lost = seen.contains { if case .unreachable = $0 { true } else { false } }
        #expect(lost == false)
        #expect(
            seen.contains {
                if case .connected(let daemon) = $0 {
                    daemon.health?.status == .degraded
                } else {
                    false
                }
            })
        await link.stop()
    }

    @Test func everyListenerStartsFromTheCurrentStatus() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon()
            _ = try? session.readPayload()
        }
        let link = makeLink()
        var early = StateWatcher(link)
        await link.start()
        let connected = await early.next { if case .connected = $0 { true } else { false } }
        var late = StateWatcher(link)
        var another = StateWatcher(link)
        #expect(await late.next { _ in true } == connected)
        #expect(await another.next { _ in true } == connected)
        await link.stop()
    }

    @Test func reconnectWhileConnectedChangesNothingAndSendsNothing() async throws {
        let server = try FakeDaemonServer(path: root.endpoint.socketPath)
        server.serve { session in
            try session.startAsDaemon()
            _ = try? session.readPayload()
        }
        let link = makeLink()
        var states = StateWatcher(link)
        await link.start()
        let connected = await states.next { if case .connected = $0 { true } else { false } }
        let before = server.bytesReceived
        await link.retryNow()
        try await Task.sleep(for: .milliseconds(150))
        #expect(link.currentState == connected)
        #expect(server.bytesReceived == before)
        await link.stop()
    }
}
