import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The live shell (``ShellBootstrap``, the real link, sync engine, and model) against a
/// real `stacklyd`: connecting once the daemon appears, the offline banner with mutations
/// disabled, reconnecting through a snapshot after a restart, and closing or quitting
/// without stopping the daemon (spec 0013 AC-7 to AC-10, AC-15; PRD AC-024).
///
/// Each test runs its own daemon under a private root. Without the binary the tests end
/// as BLOCKED (with evidence) or cancelled.
@Suite(.tags(.daemon), .timeLimit(.minutes(2)))
struct ShellEndToEndTests {
    /// The VoiceOver announcements a live shell posted.
    final class Announcements {
        var lines: [String] = []
    }

    /// A live shell for `daemon`'s root, with a close notice that shows no panel.
    static func shell(for daemon: LiveDaemon, announcements: Announcements) -> LiveShellParts {
        ShellBootstrap.assemble(
            rootOverride: daemon.root,
            clientVersion: "0.1.0",
            announce: { announcements.lines.append($0) },
            closeNotice: CloseNotice(presentsPanel: false))
    }

    /// Connected with every store refreshed, whatever the daemon's health.
    static func isRefreshed(_ model: DaemonModel) -> Bool {
        model.presentation.isConnected
    }

    /// Waits up to 30 seconds until `model` is connected with every store refreshed,
    /// after more than `rounds` snapshot rounds when given.
    static func waitUntilRefreshed(
        _ model: DaemonModel,
        _ what: String,
        pastRounds rounds: Int = -1,
        sourceLocation: SourceLocation = #_sourceLocation
    ) async throws {
        try await waitFor(
            what, seconds: 30, state: { describe(model) }, sourceLocation: sourceLocation,
            until: { isRefreshed(model) && model.sync.completedRounds > rounds })
    }

    /// Where the model stands, for a timeout message.
    static func describe(_ model: DaemonModel) -> String {
        let store = model.operations.store
        return
            "presence \(model.presence), link \(String(describing: model.connectionState)), last error \(model.lastError?.code ?? "none"), settled \(model.sync.isSettled), rounds \(model.sync.completedRounds), operations refreshing \(store.isRefreshing) unsupported \(store.unsupported) error \(store.error?.code ?? "none")"
    }

    // MARK: - Connect

    @Test(
        .evidence(
            covers: ["AC-024"],
            expected:
                "without stacklyd the live shell stays up, says it cannot connect, blocks mutations, then connects by itself once stacklyd starts"
        ))
    func theShellWaitsForStacklydThenConnectsByItself() async throws {
        let daemon = try LiveDaemon(recordsEvidence: true)
        let announcements = Announcements()
        let parts = Self.shell(for: daemon, announcements: announcements)
        let model = parts.runtime.model

        try await waitFor("the first attempt to fail", seconds: 20) {
            model.presence == .unreachable(lastKnownAt: nil)
        }
        #expect(model.lastError?.code == "ipc.endpoint_missing")
        #expect(model.presentation.footer.label == "stacklyd unavailable")
        let banner = try #require(model.presentation.banner)
        #expect(banner.title == "Can’t connect to stacklyd.")
        #expect(
            banner.message(now: Date(), calendar: .current, locale: .current, timeZone: .current)
                == "Actions are disabled until the daemon is reachable.")
        #expect(banner.actions == [.reconnect, .openDiagnostics])
        #expect(model.mutationAvailability.blockedReason == "stacklyd is unavailable.")
        #expect(model.presentation.pageFailure?.title == "Waiting for stacklyd")
        #expect(model.sidebarCounts == SidebarCounts())
        let outcome = await MutationPath(model: model).perform(.daemonGetInfo)
        guard case .notSent(let body) = outcome else {
            Issue.record("a mutation while unreachable gave \(outcome)")
            return
        }
        #expect(body.code == "ipc.endpoint_missing")

        try await daemon.start()
        // Retries run at 0.5, 1, 2, and 4 seconds, then every 5.
        try await Self.waitUntilRefreshed(model, "the shell to connect and refresh")
        #expect(model.presentation.banner == nil)
        #expect(model.mutationAvailability == .available)
        #expect(model.lastDaemon?.info?.socketPath == daemon.endpoint.socketPath)
        #expect(model.sync.lastResnapshot == .noCursor)
        #expect(
            announcements.lines == ["Can’t connect to stacklyd.", "Reconnected. State refreshed."])
        Evidence.actual(
            "unreachable with endpoint_missing, mutations not sent, then connected and refreshed after stacklyd started"
        )
        await parts.shutDown()
        await daemon.stop()
    }

    // MARK: - Offline

    @Test(
        .evidence(
            covers: ["AC-024"],
            expected:
                "after stacklyd dies the live shell keeps the last known state with its time, blocks mutations, and sends nothing"
        ))
    func aKilledDaemonLeavesTheLastKnownStateWithMutationsDisabled() async throws {
        let daemon = try LiveDaemon(recordsEvidence: true)
        try await daemon.start()
        let announcements = Announcements()
        let parts = Self.shell(for: daemon, announcements: announcements)
        let model = parts.runtime.model
        try await Self.waitUntilRefreshed(model, "the shell to connect and refresh")
        let countsBefore = model.sidebarCounts

        await daemon.kill()
        try await waitFor("the shell to see the daemon gone", seconds: 20) {
            if case .unreachable(let lastKnownAt) = model.presence {
                lastKnownAt != nil
            } else {
                false
            }
        }
        let banner = try #require(model.presentation.banner)
        #expect(banner.title == "Can’t connect to stacklyd.")
        #expect(
            banner.message(now: Date(), calendar: .current, locale: .current, timeZone: .current)
                .hasPrefix("Showing the last known state from ") == true)
        #expect(model.presentation.footer.label == "stacklyd unavailable")
        #expect(model.mutationAvailability.blockedReason == "stacklyd is unavailable.")
        // The stores keep what they had; only the connected only Operations badge hides.
        #expect(model.sidebarCounts == countsBefore)
        #expect(
            SidebarSection.operations.badge(
                counts: model.sidebarCounts, isConnected: model.presentation.isConnected) == nil)
        let outcome = await MutationPath(model: model).perform(.daemonGetInfo)
        guard case .notSent = outcome else {
            Issue.record("a mutation while unreachable gave \(outcome)")
            return
        }
        // The killed daemon left its socket behind: the next attempts are refused.
        try await waitFor("an attempt on the stale socket", seconds: 20) {
            model.lastError?.code == "ipc.connection_refused"
        }
        let rows = model.diagnostics.rows(
            calendar: .current, locale: .current, timeZone: .current)
        let values = Dictionary(uniqueKeysWithValues: rows.map { ($0.label, $0.value) })
        #expect(values["stacklyd"]?.hasSuffix("(last known)") == true)
        #expect(announcements.lines == ["Can’t connect to stacklyd."])
        Evidence.actual(
            "unreachable with the last known time, gate blocked, mutation not sent, then connection_refused on the stale socket"
        )
        await parts.shutDown()
    }

    // MARK: - Reconnect

    @Test(
        .evidence(
            covers: ["AC-024"],
            expected:
                "after stacklyd restarts the live shell catches up through a server_restarted gap and a fresh snapshot"
        ))
    func aRestartedDaemonIsCaughtUpWithASnapshotFromTheCursor() async throws {
        let daemon = try LiveDaemon(recordsEvidence: true)
        try await daemon.start()
        let announcements = Announcements()
        let parts = Self.shell(for: daemon, announcements: announcements)
        let model = parts.runtime.model
        try await Self.waitUntilRefreshed(model, "the shell to connect and refresh")
        let first = try #require(model.lastDaemon?.accepted.serverInstanceId)
        #expect(await parts.engine.cursor?.serverInstanceId == first)
        let rounds = model.sync.completedRounds

        await daemon.stop()
        try await waitFor("the shell to see the daemon stop", seconds: 20) {
            if case .unreachable = model.presence { true } else { false }
        }
        #expect(model.mutationAvailability.blockedReason == "stacklyd is unavailable.")
        if case .unreachable(let lastKnownAt) = model.presence {
            #expect(lastKnownAt != nil)
        }

        try await daemon.start()
        try await Self.waitUntilRefreshed(
            model, "the shell to reconnect and refresh", pastRounds: rounds)
        let second = try #require(model.lastDaemon?.accepted.serverInstanceId)
        #expect(second != first)
        // The stored cursor named the old run, so the daemon answered with a gap and the
        // stores loaded a full snapshot.
        #expect(model.sync.lastResnapshot == .gap(.serverRestarted))
        #expect(await parts.engine.cursor?.serverInstanceId == second)
        #expect(model.presentation.banner == nil)
        #expect(model.mutationAvailability == .available)
        #expect(
            announcements.lines == ["Can’t connect to stacklyd.", "Reconnected. State refreshed."])
        Evidence.actual(
            "reconnected to a new serverInstanceId through a server_restarted gap and a full snapshot"
        )
        await parts.shutDown()
        await daemon.stop()
    }

    // MARK: - Closing and quitting

    @Test func closingTheWindowOrQuittingLeavesTheDaemonRunning() async throws {
        let daemon = try LiveDaemon(recordsEvidence: false)
        try await daemon.start()
        let parts = Self.shell(for: daemon, announcements: Announcements())
        let runtime = parts.runtime
        try await Self.waitUntilRefreshed(runtime.model, "the shell to connect and refresh")
        let connectedAt = runtime.model.lastConnectedAt

        runtime.mainWindowWillClose(isTerminating: false, screen: nil)
        #expect(runtime.closeNotice.hasShown)
        try await Task.sleep(for: .milliseconds(300))
        #expect(Self.isRefreshed(runtime.model))
        #expect(runtime.model.lastConnectedAt == connectedAt)

        runtime.willTerminate()
        // Every wait before a new attempt passes; the link never comes back.
        try await Task.sleep(for: .seconds(1))
        if case .connecting = parts.link.currentState {
            Issue.record("the link tried again after the app quit")
        }
        #expect(daemon.isRunning)
        let client = ClientInfo(kind: .gui, version: "0.1.0", pid: 1, interactive: true)
        let (connection, _) = try await DaemonConnection.open(
            endpoint: daemon.endpoint, policy: .sameUser, client: client)
        let health = try await connection.call(.daemonGetHealth)
        #expect(health.methodName == "daemon.get_health")
        await connection.close()
        await parts.shutDown()
        await daemon.stop()
    }
}
