import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// The runtime's lifecycle hooks with a link the test counts: closing the main window and
/// quitting send nothing to the daemon, quitting closes the socket once, and the app
/// becoming active or the Mac waking retries at once (spec 0013 AC-7, AC-15).
@Suite(.timeLimit(.minutes(1)))
struct ShellRuntimeTests {
    /// A link that counts what the runtime asks of it.
    final class CountingLink: Sendable {
        let states: AsyncStream<ConnectionState>
        let push: AsyncStream<ConnectionState>.Continuation
        let calls = Mutex(0)
        let stops = Mutex(0)
        let retries = Mutex(0)

        init() {
            (states, push) = AsyncStream.makeStream()
        }

        var handle: LinkHandle {
            LinkHandle(
                states: { self.states },
                start: {},
                retryNow: { self.retries.withLock { $0 += 1 } },
                call: { _, _, _ throws(ClientError) in
                    self.calls.withLock { $0 += 1 }
                    throw .notSent(ClientErrorCatalog.notConnected())
                },
                refresh: {},
                stopNow: { self.stops.withLock { $0 += 1 } })
        }
    }

    /// A runtime over a counting link, connected, with a notice that shows no panel.
    static func connectedRuntime() async throws -> (ShellRuntime, CountingLink) {
        let link = CountingLink()
        let model = DaemonModel(
            link: link.handle, signals: nil, endpoint: nil, launchedAt: DaemonModelTests.launch)
        let runtime = ShellRuntime(model: model, closeNotice: CloseNotice(presentsPanel: false))
        let connected = ConnectionState.connected(DaemonModelTests.daemon(.ready))
        link.push.yield(connected)
        try await waitUntil { model.connectionState == connected }
        return (runtime, link)
    }

    @Test func closingTheMainWindowSendsNothingAndStopsNothing() async throws {
        let (runtime, link) = try await Self.connectedRuntime()
        let sent = link.calls.withLock { $0 }
        runtime.mainWindowWillClose(isTerminating: false, screen: nil)
        for _ in 0..<20 { await Task.yield() }
        #expect(runtime.closeNotice.hasShown)
        #expect(link.calls.withLock { $0 } == sent)
        #expect(link.stops.withLock { $0 } == 0)
        let stillConnected =
            if case .connected = runtime.model.connectionState { true } else { false }
        #expect(stillConnected, "closing the window changed the connection")
    }

    @Test func quittingClosesTheSocketOnceAndSendsNothing() async throws {
        let (runtime, link) = try await Self.connectedRuntime()
        let sent = link.calls.withLock { $0 }
        // AppKit asks to terminate before the window closes, so no notice shows.
        runtime.mainWindowWillClose(isTerminating: true, screen: nil)
        runtime.willTerminate()
        for _ in 0..<20 { await Task.yield() }
        #expect(runtime.closeNotice.hasShown == false)
        #expect(link.stops.withLock { $0 } == 1)
        #expect(link.calls.withLock { $0 } == sent)
    }

    @Test func becomingActiveOrWakingRetriesNow() async throws {
        let (runtime, link) = try await Self.connectedRuntime()
        runtime.didBecomeActive()
        runtime.didWake()
        try await waitUntil { link.retries.withLock { $0 } == 2 }
    }
}
