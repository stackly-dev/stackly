import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The daemon model with a background agent: the banner row, the hint, the code of the
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
            initialProbe: Self.fresh(.enabled), holdFirstCall: true)
        try await rig.push(Self.refusedState(5))
        // The refresh the loss starts waits at the gate, so the input is the initial probe's.
        await rig.controller.waitUntilHolding()
        let input = rig.model.agentBannerInput
        #expect(DaemonBanner.ticksEverySecond(input, now: Self.launch.addingTimeInterval(10)))
        #expect(
            DaemonBanner.ticksEverySecond(input, now: Self.launch.addingTimeInterval(20)) == false)
        rig.controller.release()
    }

    static let notFoundTitle = "Stackly can’t find its background service."

    @Test func notFoundIsLeftToTheCriticalBannerWhenTheWindowBannerShowsIt() async throws {
        let rig = Rig(afterRegister: Self.fresh(.notFound), initialProbe: Self.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        for _ in 0..<20 { await Task.yield() }

        await rig.agent?.turnOn()
        #expect(rig.agent?.assessment?.state == .notFound)

        #expect(rig.model.bannerContent(now: rig.clock)?.tone == .critical)
    }

    // MARK: - Failed actions

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
        try await rig.push(.connecting(attempt: 2))
        try await rig.push(DaemonModelTests.unreachable(125))

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

}
