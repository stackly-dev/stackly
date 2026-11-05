import Foundation
import Synchronization
import Testing

@testable import StacklyClient

/// How the connection keeps its event streams: a dropped stream stops
/// buffering, and a reused subscription ID ends the earlier stream.
@Suite(.timeLimit(.minutes(1)))
struct DaemonConnectionSubscriptionTests {
    let root: ShortTempRoot
    let server: FakeDaemonServer

    init() throws {
        root = try ShortTempRoot()
        server = try FakeDaemonServer(path: root.endpoint.socketPath)
    }

    func open() async throws -> DaemonConnection {
        try await DaemonConnection.open(
            endpoint: root.endpoint, policy: .sameUser, client: Samples.client,
            options: CallOptions(timeout: .seconds(5))
        ).0
    }

    static let head = EventCursor(serverInstanceId: Samples.serverInstanceId, sequence: 0)

    static func event(_ subscriptionId: UInt32, sequence: UInt64) -> ServerFrame {
        .event(
            Event(
                subscriptionId: subscriptionId, sequence: sequence,
                at: Date(timeIntervalSince1970: 0), resource: nil, revision: nil,
                kind: .eventsGap(Gap(reason: .overflow))))
    }

    @Test func aDroppedStreamIsForgottenSoItStopsBufferingFrames() async throws {
        let done = DispatchSemaphore(value: 0)
        server.serve { session in
            try session.acceptHandshake()
            let request = try session.readRequest()
            try session.reply(
                to: request, with: .eventsSubscribe(Subscribed(subscriptionId: 7, head: Self.head)))
            try done.waitForTest("the stream was dropped")
            session.close()
        }
        defer { done.signal() }
        let connection = try await open()
        do {
            let (subscribed, stream) = try await connection.subscribe(
                SubscribeParams(since: nil, kinds: [], resource: nil))
            #expect(subscribed.subscriptionId == 7)
            #expect(await connection.activeSubscriptionCount == 1)
            _ = stream
        }
        // The stream's end runs on its own task: wait for it without a clock.
        var remaining = 10_000
        while await connection.activeSubscriptionCount > 0, remaining > 0 {
            remaining -= 1
            await Task.yield()
        }
        #expect(await connection.activeSubscriptionCount == 0)
    }

    @Test func aReusedSubscriptionIdEndsTheEarlierStream() async throws {
        let second = DispatchSemaphore(value: 0)
        server.serve { session in
            try session.acceptHandshake()
            let first = try session.readRequest()
            try session.reply(
                to: first, with: .eventsSubscribe(Subscribed(subscriptionId: 7, head: Self.head)))
            try second.waitForTest("the first stream was taken")
            let again = try session.readRequest()
            try session.reply(
                to: again, with: .eventsSubscribe(Subscribed(subscriptionId: 7, head: Self.head)))
            try session.send(Self.event(7, sequence: 1))
            session.close()
        }
        let connection = try await open()
        let params = SubscribeParams(since: nil, kinds: [], resource: nil)
        let (_, earlier) = try await connection.subscribe(params)
        second.signal()
        let (_, later) = try await connection.subscribe(params)

        var earlierFrames: [DecodedFrame] = []
        for try await frame in earlier { earlierFrames.append(frame) }
        #expect(earlierFrames.isEmpty)

        var laterFrames: [DecodedFrame] = []
        do {
            for try await frame in later { laterFrames.append(frame) }
        } catch {}
        #expect(laterFrames.count == 1)
    }
}
