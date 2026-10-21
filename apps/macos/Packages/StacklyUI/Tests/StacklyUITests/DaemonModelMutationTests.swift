import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The mutation path in every connection state, a refused socket or protocol at
/// the model level, and Diagnostics once the daemon is back (spec 0013 AC-11,
/// AC-12, AC-19).
extension DaemonModelTests {
    // MARK: - Mutation path

    @Test func everyUserActionGetsAFreshLowercaseKey() async throws {
        let fixture = Fixture()
        try await fixture.push(.connected(Self.daemon(.ready)))
        let path = MutationPath(model: fixture.model)
        _ = await path.perform(.daemonGetInfo)
        _ = await path.perform(.daemonGetInfo)
        let keys = fixture.link.calls.withLock { $0.compactMap { $0.1 } }
        #expect(keys.count == 2)
        #expect(Set(keys).count == 2)
        for key in keys {
            #expect(key == key.lowercased())
            #expect(UUID(uuidString: key) != nil)
        }
    }

    @Test func aRetryAfterAnUnknownOutcomeSendsTheKeyOfThatAction() async throws {
        let fixture = Fixture()
        try await fixture.push(.connected(Self.daemon(.ready)))
        let path = MutationPath(model: fixture.model)
        let lost = ClientErrorCatalog.connectionClosed()
        fixture.link.result.withLock { $0 = .failure(.outcomeUnknown(lost)) }
        guard case .outcomeUnknown(let error, let key) = await path.perform(.daemonGetInfo) else {
            Issue.record("expected an unknown outcome")
            return
        }
        #expect(error == lost)
        fixture.link.result.withLock { $0 = .success(.eventsUnsubscribe) }
        #expect(
            await path.perform(.daemonGetInfo, idempotencyKey: key)
                == .completed(.eventsUnsubscribe))
        let sent = fixture.link.calls.withLock { $0.compactMap { $0.1 } }
        #expect(sent == [key, key])
    }

    @Test func aTimedOutMutationKeepsWhetherItsFrameWasWritten() async throws {
        let fixture = Fixture()
        try await fixture.push(.connected(Self.daemon(.ready)))
        let path = MutationPath(model: fixture.model)
        let timeout = ClientErrorCatalog.callTimeout(
            method: "daemon.get_info", limit: .seconds(10), idempotencyKey: "k9")
        fixture.link.result.withLock { $0 = .failure(.notSent(timeout)) }
        #expect(await path.perform(.daemonGetInfo, idempotencyKey: "k9") == .notSent(timeout))
        fixture.link.result.withLock { $0 = .failure(.outcomeUnknown(timeout)) }
        #expect(
            await path.perform(.daemonGetInfo, idempotencyKey: "k9")
                == .outcomeUnknown(timeout, key: "k9"))
    }

    @Test func noMutationReachesTheLinkWhileTheStateIsNotConnected() async throws {
        let untrusted = ClientErrorCatalog.endpointUntrusted(path: "/p", reason: .symlink)
        let rejected = Samples.error("request.invalid_params")
        let cases: [(ConnectionState, ErrorBody)] = [
            (.connecting(attempt: 1), ClientErrorCatalog.notConnected()),
            (Self.unreachable(), Self.missing),
            (.untrusted(untrusted, since: Self.launch, nextAttempt: Self.launch), untrusted),
            (
                .incompatible(
                    .handshakeRejected(rejected), since: Self.launch, nextAttempt: Self.launch),
                rejected
            ),
            (.misconfigured(.invalidClientVersion), ClientErrorCatalog.notConnected()),
        ]
        for (state, expected) in cases {
            let fixture = Fixture()
            try await fixture.push(state)
            let outcome = await MutationPath(model: fixture.model).perform(.daemonGetInfo)
            #expect(outcome == .notSent(expected))
            #expect(fixture.link.calls.withLock { $0.isEmpty })
        }
    }

    // MARK: - Refused socket and protocol

    @Test(
        .evidence(
            covers: ["AC-070"],
            expected:
                "when the socket fails a security check the shell says so, blocks mutations, and sends nothing"
        ))
    func anUntrustedSocketShowsTheCriticalBannerAndSendsNothing() async throws {
        let fixture = Fixture()
        let model = fixture.model!
        let untrusted = ClientErrorCatalog.endpointUntrusted(
            path: "/t/stackly/stacklyd.sock", reason: .symlink)
        try await fixture.push(
            .untrusted(
                untrusted, since: Self.launch.addingTimeInterval(5), nextAttempt: Self.launch))
        #expect(model.presence == .untrusted(reason: "symlink", path: "/t/stackly/stacklyd.sock"))
        #expect(model.presentation.footer.label == "stacklyd untrusted")
        let banner = try #require(model.presentation.banner)
        #expect(banner.tone == .critical)
        #expect(banner.title == "Stackly didn’t connect to stacklyd.")
        #expect(
            banner.message(
                now: Self.launch, calendar: .current, locale: .current, timeZone: .current)
                == "The socket failed a security check: the path is a symbolic link. Nothing was sent."
        )
        #expect(banner.actions == [.reconnect, .openDiagnostics])
        #expect(
            model.mutationAvailability.blockedReason == "stacklyd’s socket failed a security check."
        )
        #expect(model.presentation.pageFailure?.code == "ipc.endpoint_untrusted")
        let outcome = await MutationPath(model: model).perform(.daemonGetInfo)
        #expect(outcome == .notSent(untrusted))
        #expect(fixture.link.calls.withLock { $0.isEmpty })
    }

    @Test func anotherMajorShowsTheCriticalBannerBlocksMutationsAndNamesBothProtocols()
        async throws
    {
        let fixture = Fixture()
        let model = fixture.model!
        let mismatch = Samples.error("protocol.major_mismatch")
        try await fixture.push(
            .incompatible(
                .majorMismatch(server: ProtocolVersion(major: 2, minor: 0), error: mismatch),
                since: Self.launch.addingTimeInterval(5), nextAttempt: Self.launch))
        let banner = try #require(model.presentation.banner)
        #expect(model.presentation.footer.label == "stacklyd incompatible")
        #expect(banner.tone == .critical)
        #expect(banner.title == "This version of Stackly can’t talk to stacklyd.")
        #expect(
            model.mutationAvailability.blockedReason
                == "This version of Stackly can’t talk to stacklyd.")
        let rows = model.diagnostics.rows(
            calendar: Calendar(identifier: .gregorian), locale: Locale(identifier: "en_US"),
            timeZone: .gmt)
        let values = Dictionary(uniqueKeysWithValues: rows.map { ($0.label, $0.value) })
        #expect(values["Protocol"] == "stacklyd 2.0 · Stackly \(ShellStatusMapping.appProtocol)")
        #expect(values["What to do"] == "Install matching versions of Stackly and stacklyd.")
        #expect(await MutationPath(model: model).perform(.daemonGetInfo) == .notSent(mismatch))
    }

    // MARK: - Diagnostics after a loss

    @Test func diagnosticsDropTheOutageRowsOnceTheDaemonIsBack() async throws {
        let fixture = Fixture()
        let model = fixture.model!
        func values() -> [String: String] {
            let rows = model.diagnostics.rows(
                calendar: Calendar(identifier: .gregorian), locale: Locale(identifier: "en_US"),
                timeZone: .gmt)
            return Dictionary(uniqueKeysWithValues: rows.map { ($0.label, $0.value) })
        }
        try await fixture.push(.connected(Self.daemon(.degraded)))
        await fixture.send(.resnapshot(.noCursor))
        try await waitUntil { model.presence == .connected(health: .degraded) }
        let firstConnected = try #require(values()["Last connected"])
        try await fixture.push(Self.unreachable(40))
        let lost = values()
        #expect(lost["Next attempt"] != nil)
        #expect(lost["What to do"] != nil)
        #expect(lost["stacklyd"]?.hasSuffix("(last known)") == true)

        try await fixture.push(.connected(Self.daemon(.degraded, at: 50)))
        await fixture.send(.caughtUp)
        try await waitUntil { model.presence == .connected(health: .degraded) }
        let back = values()
        #expect(back["Status"] == "stacklyd degraded")
        #expect(back["Next attempt"] == nil)
        #expect(back["stacklyd"]?.hasSuffix("(last known)") == false)
        #expect(back["Last connected"] != firstConnected)
    }

    /// A real finding, left failing on purpose: `ConnectionHint.text` falls back to the code
    /// hint of the retained `lastError` even when the status is connected, so Diagnostics
    /// keeps saying "stacklyd isn’t running" while the daemon runs.
    @Test func aConnectedDaemonShowsNoWhatToDoRowForAnOldError() async throws {
        let fixture = Fixture()
        let model = fixture.model!
        try await fixture.push(.connected(Self.daemon(.ready)))
        await fixture.send(.resnapshot(.noCursor))
        try await fixture.push(Self.unreachable(40))
        try await fixture.push(.connected(Self.daemon(.ready, at: 50)))
        await fixture.send(.caughtUp)
        try await waitUntil { model.presence == .connected(health: .ready) }
        let rows = model.diagnostics.rows(
            calendar: Calendar(identifier: .gregorian), locale: Locale(identifier: "en_US"),
            timeZone: .gmt)
        let values = Dictionary(uniqueKeysWithValues: rows.map { ($0.label, $0.value) })
        #expect(values["Status"] == "stacklyd running")
        #expect(values["What to do"] == nil, "got: \(values["What to do"] ?? "")")
    }
}
