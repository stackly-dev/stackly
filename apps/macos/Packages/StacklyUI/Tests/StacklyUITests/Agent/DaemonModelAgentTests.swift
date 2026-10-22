import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The daemon model with a background agent: the banner row, the hint, the code of the
/// last failure, and the announcements (spec 0015, *Banner, pengumuman, dan environment*,
/// AC-13; spec 0013, AC-21). Every wait goes through `ManualSleep`.
@Suite(.timeLimit(.minutes(1)))
struct DaemonModelAgentTests {
    static let launch = DaemonModelTests.launch
    static let offTitle = "Stackly’s background service is off."
    static let lost = "Can’t connect to stacklyd."

    /// A model with an agent over fakes, a fake link, and a manual `sleep`.
    final class Rig {
        let link = DaemonModelTests.FakeLink()
        let signals: AsyncStream<SyncSignal>
        let signal: AsyncStream<SyncSignal>.Continuation
        let controller = FakeBackgroundAgentController()
        let sleeper = ManualSleep()
        var announced: [String] = []
        var clock = DaemonModelTests.launch
        let agent: BackgroundAgentModel?
        var model: DaemonModel!

        init(
            status: Result<AgentProbe, AgentActionError>? = nil,
            afterRegister: Result<AgentProbe, AgentActionError>? = nil,
            record: LaunchRecordStatus = .missing,
            withAgent: Bool = true,
            initialProbe: Result<AgentProbe, AgentActionError>? = nil,
            holdFirstCall: Bool = false
        ) {
            (signals, signal) = AsyncStream.makeStream()
            if let status { controller.setStatus(status) }
            if let afterRegister { controller.setAfterRegister(afterRegister) }
            if holdFirstCall { controller.holdNextCall() }
            agent =
                withAgent
                ? BackgroundAgentModel(
                    controller: controller, launchRecord: FakeLaunchRecordReader(record),
                    rootOverrideActive: false, initialProbe: initialProbe,
                    initialLaunchRecord: record) : nil
            let sleeper = sleeper
            model = DaemonModel(
                link: link.handle, signals: signals,
                endpoint: SocketEndpoint(
                    runtimeRootBytes: Array("/t/stackly".utf8), rootOverride: false),
                launchedAt: DaemonModelAgentTests.launch,
                now: { [weak self] in self?.clock ?? DaemonModelAgentTests.launch },
                sleep: { try await sleeper.sleep(for: $0) },
                announce: { [weak self] in self?.announced.append($0) },
                agent: agent)
            model.start()
        }

        func push(_ state: ConnectionState) async throws {
            link.push.yield(state)
            try await waitUntil { self.model.connectionState == state }
        }

        func send(_ value: SyncSignal) async {
            signal.yield(value)
            for _ in 0..<20 { await Task.yield() }
        }

        /// Suspends until a sleep of `duration` waits.
        func waitForSleep(_ duration: Duration) async {
            while sleeper.durations.contains(duration) == false { await Task.yield() }
            await sleeper.waitUntilSleeping()
        }
    }

    static func fresh(_ registration: AgentRegistration = .notFound) -> Result<
        AgentProbe, AgentActionError
    > {
        .success(.registration(registration, bundle: FakeBackgroundAgentController.fixtureBundle))
    }

    static let refused = ClientErrorCatalog.connectionRefused(path: "/t/stackly/stacklyd.sock")

    static func refusedState(_ seconds: TimeInterval = 5) -> ConnectionState {
        .unreachable(
            refused, since: launch.addingTimeInterval(seconds),
            nextAttempt: launch.addingTimeInterval(seconds + 0.5))
    }

    // MARK: - Without an agent

    @Test func withoutAnAgentTheBannerIsTheStatusBannerAndTheHintIsEmpty() async throws {
        let rig = Rig(withAgent: false)
        try await rig.push(DaemonModelTests.unreachable())
        #expect(rig.model.agent == nil)
        #expect(rig.model.agentBannerInput == nil)
        #expect(rig.model.bannerContent(now: rig.clock) == rig.model.presentation.banner)
        #expect(rig.model.diagnostics.agentHint == nil)
        #expect(rig.announced == [Self.lost])
        let rows = rig.model.diagnostics.rows(
            calendar: .current, locale: .current, timeZone: .current)
        #expect(rows.contains { $0.label == "What to do" } == false)
    }

    // MARK: - The banner row

    @Test func anUnreachableDaemonWithAnAgentShowsTheAgentRowAndTheHint() async throws {
        let rig = Rig(initialProbe: Self.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        let banner = try #require(rig.model.bannerContent(now: rig.clock))
        #expect(banner.title == Self.offTitle)
        #expect(banner.actions == [.turnOn, .openDiagnostics])
        #expect(rig.model.presentation.banner?.title == Self.lost)
        #expect(rig.model.agentBannerInput?.failureCode == "ipc.endpoint_missing")
        let rows = rig.model.diagnostics.rows(
            calendar: .current, locale: .current, timeZone: .current, now: rig.clock)
        let hint = try #require(rows.first { $0.label == "What to do" })
        #expect(hint.value.hasPrefix(Self.offTitle))
        #expect(rig.model.copyDiagnostics(now: rig.clock).contains("What to do: \(hint.value)"))
    }

    @Test func bothCodesGiveTheAgentRowAndOtherCodesGiveTheStatusBanner() async throws {
        let rig = Rig(initialProbe: Self.fresh())
        try await rig.push(Self.refusedState())
        #expect(rig.model.bannerContent(now: rig.clock)?.title == Self.offTitle)
        let other = ErrorBody(
            code: "ipc.io_error", category: .unavailable, message: "m", retryable: true,
            resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
            items: nil)
        try await rig.push(.unreachable(other, since: Self.launch, nextAttempt: Self.launch))
        #expect(rig.model.bannerContent(now: rig.clock)?.title == Self.lost)
        #expect(rig.model.diagnostics.agentHint == nil)
    }

    @Test func theCodeFollowsTheLastFailedAttemptAndStaysWhileRefreshing() async throws {
        let rig = Rig(initialProbe: Self.fresh())
        #expect(rig.model.unreachableCode == nil)
        try await rig.push(DaemonModelTests.unreachable())
        #expect(rig.model.unreachableCode == "ipc.endpoint_missing")
        try await rig.push(.connecting(attempt: 2))
        #expect(rig.model.unreachableCode == "ipc.endpoint_missing")
        try await rig.push(Self.refusedState(6))
        #expect(rig.model.unreachableCode == "ipc.connection_refused")
        try await rig.push(.connected(DaemonModelTests.daemon(.ready, at: 20)))
        #expect(rig.model.presence == .refreshing)
        #expect(rig.model.unreachableCode == "ipc.connection_refused")
        await rig.send(.caughtUp)
        try await waitUntil { rig.model.presence == .connected(health: .ready) }
        #expect(rig.model.unreachableCode == nil)
    }

    @Test func theRowStaysWhileRefreshingButTheHintGoes() async throws {
        let rig = Rig(initialProbe: Self.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        try await rig.push(.connected(DaemonModelTests.daemon(.ready, at: 20)))
        #expect(rig.model.presence == .refreshing)
        #expect(rig.model.bannerContent(now: rig.clock)?.title == Self.offTitle)
        #expect(rig.model.diagnostics.agentHint == nil)
        let rows = rig.model.diagnostics.rows(
            calendar: .current, locale: .current, timeZone: .current, now: rig.clock)
        #expect(rows.contains { $0.label == "What to do" } == false)
        await rig.send(.caughtUp)
        try await waitUntil { rig.model.presence == .connected(health: .ready) }
        #expect(rig.model.bannerContent(now: rig.clock) == nil)
    }

    @Test func theScheduleTicksForTheModelInput() async throws {
        let rig = Rig(
            record: LaunchRecordStatus(exists: true, lastLaunchedAt: nil),
            initialProbe: Self.fresh(.enabled))
        try await rig.push(Self.refusedState(5))
        let input = rig.model.agentBannerInput
        #expect(DaemonBanner.ticksEverySecond(input, now: Self.launch.addingTimeInterval(10)))
        #expect(
            DaemonBanner.ticksEverySecond(input, now: Self.launch.addingTimeInterval(20)) == false)
    }

    // MARK: - Announcements

    @Test func aLossWithAFreshStateAnnouncesTheRowTitleOnce() async throws {
        let rig = Rig(initialProbe: Self.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        for _ in 0..<20 { await Task.yield() }
        #expect(rig.announced == [Self.offTitle])
        try await rig.push(.connecting(attempt: 2))
        try await rig.push(DaemonModelTests.unreachable(6))
        #expect(rig.announced == [Self.offTitle])
    }

    @Test func aLossWithAnOldStateWaitsForTheFirstProbeAndAnnouncesItsTitle() async throws {
        let rig = Rig(initialProbe: Self.fresh())
        rig.clock = Self.launch.addingTimeInterval(600)
        try await rig.push(DaemonModelTests.unreachable(600))
        // The status is older than a poll, so nothing is said until a probe arrives.
        #expect(rig.announced.isEmpty)
        try await waitUntil { rig.controller.count(of: .status) == 1 }
        try await waitUntil { rig.announced == [Self.offTitle] }
        #expect(rig.announced == [Self.offTitle])
    }

    @Test func aLossBeforeAnyStateWaitsForTheFirstProbeAndAnnouncesItsTitle() async throws {
        let rig = Rig(holdFirstCall: true)
        try await rig.push(DaemonModelTests.unreachable())
        await rig.controller.waitUntilHolding()
        #expect(rig.announced.isEmpty)
        rig.controller.release()
        try await waitUntil { rig.announced == [Self.offTitle] }
        // The state the probe found is recorded, not announced again as a change.
        for _ in 0..<30 { await Task.yield() }
        #expect(rig.announced == [Self.offTitle])
    }

    @Test func aPendingLossWithoutAProbeSaysCantConnectAfterAnnounceWait() async throws {
        let rig = Rig(status: Self.fresh())
        try await waitUntil { rig.agent?.assessment != nil }
        rig.controller.holdNextCall()
        rig.clock = Self.launch.addingTimeInterval(600)
        try await rig.push(DaemonModelTests.unreachable(600))
        await rig.waitForSleep(AgentTiming.announceWait)
        #expect(rig.announced.isEmpty)
        rig.sleeper.fire()
        try await waitUntil { rig.announced == [Self.lost] }
        rig.controller.release()
        for _ in 0..<30 { await Task.yield() }
        #expect(rig.announced == [Self.lost])
    }

    @Test func aPendingLossThatReconnectsFirstIsSettledAndTheReconnectIsTheNextOne() async throws {
        let rig = Rig(status: Self.fresh())
        try await waitUntil { rig.agent?.assessment != nil }
        rig.controller.holdNextCall()
        rig.clock = Self.launch.addingTimeInterval(600)
        try await rig.push(DaemonModelTests.unreachable(600))
        #expect(rig.announced.isEmpty)
        try await rig.push(.connected(DaemonModelTests.daemon(.ready, at: 700)))
        // The assessment from before the loss gives the title the row had.
        #expect(rig.announced == [Self.offTitle])
        await rig.send(.caughtUp)
        try await waitUntil { rig.model.presence == .connected(health: .ready) }
        #expect(rig.announced == [Self.offTitle, "Reconnected. State refreshed."])
        rig.controller.release()
    }

    @Test func aChangeOfStateIsAnnouncedOnceAndTheFirstStateIsNot() async throws {
        let rig = Rig(initialProbe: Self.fresh())
        try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
        await rig.send(.caughtUp)
        try await waitUntil { rig.model.presence == .connected(health: .ready) }
        #expect(rig.announced.isEmpty)
        await rig.agent?.turnOn()
        #expect(rig.announced == ["Background service needs your approval."])
        await rig.agent?.refresh()
        #expect(rig.announced == ["Background service needs your approval."])
        rig.controller.setStatus(Self.fresh(.enabled))
        await rig.agent?.refresh()
        #expect(rig.announced.last == "Background service is on.")
    }

    static let notFoundTitle = "Stackly can’t find its background service."

    @Test func notFoundIsLeftToTheCriticalBannerWhenTheWindowBannerShowsIt() async throws {
        let rig = Rig(afterRegister: Self.fresh(.notFound), initialProbe: Self.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        for _ in 0..<20 { await Task.yield() }
        #expect(rig.announced == [Self.offTitle])
        await rig.agent?.turnOn()
        #expect(rig.agent?.assessment?.state == .notFound)
        #expect(rig.announced == [Self.offTitle])
        #expect(rig.model.bannerContent(now: rig.clock)?.tone == .critical)
    }

    @Test func notFoundIsAnnouncedOnceWhenTheWindowBannerDoesNotShowTheAgentRow() async throws {
        let rig = Rig(afterRegister: Self.fresh(.notFound), initialProbe: Self.fresh())
        try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
        await rig.send(.caughtUp)
        try await waitUntil { rig.model.presence == .connected(health: .ready) }
        let row = rig.model.agentBannerInput.flatMap { AgentBanner.content($0, now: rig.clock) }
        #expect(row == nil)
        await rig.agent?.turnOn()
        #expect(rig.agent?.assessment?.state == .notFound)
        #expect(rig.announced == [Self.notFoundTitle])
        // The same state again is not a change.
        await rig.agent?.refresh()
        #expect(rig.announced == [Self.notFoundTitle])
        // A later change is announced as usual.
        rig.controller.setStatus(Self.fresh(.requiresApproval))
        await rig.agent?.refresh()
        #expect(rig.announced.last == "Background service needs your approval.")
    }

    // MARK: - Failed actions

    @Test func aFailedActionIsAnnouncedWithTheLastErrorTextOnce() async throws {
        let rig = Rig(initialProbe: Self.fresh())
        try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
        await rig.send(.caughtUp)
        try await waitUntil { rig.model.presence == .connected(health: .ready) }
        let error = AgentActionError.os(
            op: .register, kind: .invalidSignature, domain: "d", code: 3)
        rig.controller.setAfterRegister(.failure(error))
        rig.controller.setStatus(Self.fresh())
        await rig.agent?.turnOn()
        #expect(rig.agent?.lastError == error)
        #expect(rig.announced == [error.message])
        // The next action clears the error and says nothing about it.
        rig.controller.setAfterRegister(Self.fresh(.requiresApproval))
        await rig.agent?.turnOn()
        #expect(rig.agent?.lastError == nil)
        #expect(rig.announced == [error.message, "Background service needs your approval."])
    }

    @Test func aRegistrationTheOSDidNotKeepIsAnnouncedOnce() async throws {
        let rig = Rig(afterRegister: Self.fresh(.notRegistered), initialProbe: Self.fresh())
        try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
        await rig.send(.caughtUp)
        try await waitUntil { rig.model.presence == .connected(health: .ready) }
        await rig.agent?.turnOn()
        #expect(rig.agent?.lastError == .registrationNotKept)
        #expect(rig.announced.last == AgentActionError.registrationNotKept.message)
        let kept = AgentActionError.registrationNotKept.message
        #expect(rig.announced.filter { $0 == kept }.count == 1)
    }

    @Test func daemonDidNotStartIsAnnouncedWhereverTheActionWasPressed() async throws {
        let rig = Rig(
            status: Self.fresh(.enabled), afterRegister: Self.fresh(.enabled),
            record: LaunchRecordStatus(exists: true, lastLaunchedAt: nil),
            initialProbe: Self.fresh(.enabled))
        try await rig.push(DaemonModelTests.unreachable(5))
        let action = Task { await rig.agent?.start() }
        while rig.agent?.isStarting != true { await Task.yield() }
        await rig.sleeper.waitUntilSleeping(2)  // the poll and the wait
        rig.sleeper.fire()
        await action.value
        let text = AgentActionError.daemonDidNotStart(seconds: 30).message
        #expect(rig.announced.filter { $0 == text }.count == 1)
        #expect(text == "stacklyd didn’t start within 30 seconds.")
    }

    // MARK: - Starting

    @Test func startingKeepsTheStartingRowThroughRefreshingAndRetriesQuietly() async throws {
        let rig = Rig(
            status: Self.fresh(.enabled), afterRegister: Self.fresh(.enabled),
            record: LaunchRecordStatus(exists: true, lastLaunchedAt: nil),
            initialProbe: Self.fresh(.enabled))
        try await rig.push(DaemonModelTests.unreachable(5))
        rig.clock = Self.launch.addingTimeInterval(120)
        #expect(rig.model.bannerContent(now: rig.clock)?.title == "stacklyd isn’t running.")
        let action = Task { await rig.agent?.start() }
        while rig.agent?.isStarting != true { await Task.yield() }
        #expect(rig.model.bannerContent(now: rig.clock)?.title == "stacklyd is starting…")
        try await waitUntil { rig.link.retries.withLock { $0 } == 1 }
        // A failed attempt during the wait is not announced as "Still unreachable.".
        try await rig.push(.connecting(attempt: 2))
        try await rig.push(DaemonModelTests.unreachable(125))
        #expect(rig.announced.contains("Still unreachable.") == false)
        #expect(rig.model.bannerContent(now: rig.clock)?.title == "stacklyd is starting…")
        try await rig.push(.connected(DaemonModelTests.daemon(.ready, at: 130)))
        #expect(rig.model.presence == .refreshing)
        #expect(rig.model.bannerContent(now: rig.clock)?.title == "stacklyd is starting…")
        #expect(rig.agent?.isStarting == true)
        await rig.send(.caughtUp)
        try await waitUntil { rig.model.presence == .connected(health: .ready) }
        await action.value
        #expect(rig.agent?.isStarting == false)
        #expect(rig.agent?.isWorking == false)
        #expect(rig.model.bannerContent(now: rig.clock) == nil)
    }

    // MARK: - Stopping

    @Test func stoppingTheModelCancelsThePendingAnnouncementAndTheAgent() async throws {
        let rig = Rig(status: Self.fresh())
        try await waitUntil { rig.agent?.assessment != nil }
        rig.controller.holdNextCall()
        rig.clock = Self.launch.addingTimeInterval(600)
        try await rig.push(DaemonModelTests.unreachable(600))
        await rig.waitForSleep(AgentTiming.announceWait)
        rig.model.stop()
        rig.controller.release()
        try await waitUntil { rig.sleeper.waiting == 0 }
        rig.sleeper.fire()
        for _ in 0..<30 { await Task.yield() }
        #expect(rig.announced.isEmpty)
        #expect(rig.sleeper.waiting == 0)
    }
}
