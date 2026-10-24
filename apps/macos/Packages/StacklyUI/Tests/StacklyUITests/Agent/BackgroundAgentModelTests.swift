import Foundation
import Testing

@testable import StacklyUI

/// The background agent model over fakes: the probe step, the actions, the wait for the
/// daemon, the polling, and the stop (spec 0015, AC-11, AC-13). Every wait goes through
/// `ManualSleep`, so no test waits for real time.
@Suite(.timeLimit(.minutes(1)))
struct BackgroundAgentModelTests {
    static let bundle = FakeBackgroundAgentController.fixtureBundle
    static let start = Date(timeIntervalSince1970: 1_792_300_000)

    /// One model, its fakes, and the closures `attach` gets.
    final class Rig {
        let controller: FakeBackgroundAgentController
        let record: FakeLaunchRecordReader
        let sleeper = ManualSleep()
        let model: BackgroundAgentModel
        var clock = BackgroundAgentModelTests.start
        var retries = 0
        /// `assessed` and `failed` in the order the closures ran: "assessed" or "failed".

        init(
            status: Result<AgentProbe, AgentActionError>? = nil,
            afterRegister: Result<AgentProbe, AgentActionError>? = nil,
            record: LaunchRecordStatus = .missing,
            rootOverride: Bool = false,
            initialProbe: Result<AgentProbe, AgentActionError>? = nil
        ) {
            controller = FakeBackgroundAgentController()
            if let status { controller.setStatus(status) }
            if let afterRegister { controller.setAfterRegister(afterRegister) }
            self.record = FakeLaunchRecordReader(record)
            model = BackgroundAgentModel(
                controller: controller, launchRecord: self.record,
                rootOverrideActive: rootOverride, initialProbe: initialProbe,
                initialLaunchRecord: record)
        }

        func attach(_ presence: DaemonPresence = .unreachable(lastKnownAt: nil)) {
            let sleeper = sleeper
            model.attach(
                presence: presence,
                now: { [weak self] in self?.clock ?? BackgroundAgentModelTests.start },
                sleep: { try await sleeper.sleep(for: $0) },

                systemRetry: { [weak self] in self?.retries += 1 })
        }

        /// Attaches and waits until the launch refresh finished.
        func attachAndSettle(_ presence: DaemonPresence = .connecting(slow: false)) async throws {
            attach(presence)
            try await waitUntil { self.model.assessment != nil }
        }
    }

    static func probe(_ registration: AgentRegistration) -> Result<AgentProbe, AgentActionError> {
        .success(.registration(registration, bundle: bundle))
    }

    // MARK: - Reading

    @Test func theLaunchRefreshReadsTheStatusAndNeverRegisters() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        #expect(rig.model.assessment?.state == .notRegistered)
        #expect(rig.model.assessment?.recovery == .register)
        #expect(rig.controller.calls == [.status])
        #expect(rig.model.lastCheckedAt == Self.start)
        #expect(rig.model.bundlePath == Self.bundle)
    }

    @Test func anInitialProbeIsAppliedAtOnceWithNoCallAndNoReport() {
        let rig = Rig(
            record: LaunchRecordStatus(exists: true, lastLaunchedAt: Self.start),
            initialProbe: Self.probe(.requiresApproval))
        rig.attach(.connecting(slow: false))
        #expect(rig.model.assessment?.state == .disabled)
        #expect(rig.model.lastLaunchedAt == Self.start)
        #expect(rig.controller.calls.isEmpty)
        #expect(rig.record.reads == 0)
        #expect(rig.model.justRegistered == false)
    }

    @Test func aTimedOutInitialProbeGivesTheDetailWithoutAnyStatusCall() {
        let rig = Rig(initialProbe: .failure(.timedOut(op: .status)))
        rig.attach(.connecting(slow: false))
        #expect(rig.model.assessment?.reason == .statusFailed)
        #expect(rig.model.unavailableDetail == "timed out")
        #expect(rig.controller.calls.isEmpty)
    }

    @Test func theDetailFollowsTheLastProbe() async throws {
        let cases: [(Result<AgentProbe, AgentActionError>, String?)] = [
            (.failure(.timedOut(op: .status)), "timed out"),
            (.failure(.os(op: .status, kind: .other, domain: "d", code: 5)), "error 5"),
            (Self.probe(.unrecognized(7)), "unknown status 7"),
            (Self.probe(.enabled), nil),
            (.success(.notBundled(executable: "/x")), nil),
        ]
        for (answer, detail) in cases {
            let rig = Rig(status: answer)
            try await rig.attachAndSettle()
            #expect(rig.model.unavailableDetail == detail)
        }
    }

    @Test func theBundlePathFollowsTheProbe() async throws {
        let rig = Rig(status: .success(.helperMissing(bundle: "/some/Stackly.app")))
        try await rig.attachAndSettle()
        #expect(rig.model.bundlePath == "/some/Stackly.app")
        rig.controller.setStatus(.success(.notBundled(executable: "/x/stackly-cli")))
        await rig.model.refresh()
        #expect(rig.model.bundlePath == nil)
    }

    @Test func theLastLaunchTimeComesFromTheRecord() async throws {
        let rig = Rig(
            status: Self.probe(.enabled),
            record: LaunchRecordStatus(exists: true, lastLaunchedAt: Self.start))
        try await rig.attachAndSettle()
        #expect(rig.model.lastLaunchedAt == Self.start)
    }

    @Test func rootOverrideNeverCallsTheOSOrTheRecordAndNeverPolls() async {
        let rig = Rig(rootOverride: true)
        rig.attach(.unreachable(lastKnownAt: nil))
        await rig.model.refresh()
        await rig.model.turnOn()
        await rig.model.start()
        rig.model.presenceDidChange(.unreachable(lastKnownAt: nil))
        #expect(rig.model.assessment?.reason == .rootOverride)
        #expect(rig.controller.calls.isEmpty)
        #expect(rig.record.reads == 0)
        #expect(rig.sleeper.durations.isEmpty)
    }

    @Test func aRefreshThatFailsDoesNotTouchTheLastError() async throws {
        let rig = Rig(status: .failure(.timedOut(op: .status)))
        try await rig.attachAndSettle()
        await rig.model.refresh()
        #expect(rig.model.lastError == nil)
        #expect(rig.model.assessment?.state == .unavailable)
    }

    @Test func refreshesThatOverlapJoinOne() async throws {
        let rig = Rig(status: Self.probe(.enabled))
        rig.controller.holdNextCall()
        rig.attach(.connecting(slow: false))
        await rig.controller.waitUntilHolding()
        async let second: Void = rig.model.refresh()
        rig.controller.release()
        await second
        try await waitUntil { rig.model.assessment != nil }
        #expect(rig.controller.count(of: .status) == 1)
    }

    // MARK: - Actions

    @Test func turnOnRegistersAndAFreshApprovalOpensLoginItemsOnce() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        await rig.model.turnOn()
        #expect(rig.controller.calls == [.status, .register, .openApprovalSettings])
        #expect(rig.model.assessment?.state == .requiresApproval)
        #expect(rig.model.justRegistered)
        #expect(rig.model.isWorking == false)
        #expect(rig.model.lastError == nil)
        await rig.model.refresh()
        #expect(rig.model.assessment?.state == .requiresApproval)
        #expect(rig.controller.count(of: .openApprovalSettings) == 1)
        #expect(rig.model.justRegistered)
    }

    @Test func notFoundBeforeTurnOnIsOffAndAfterTurnOnIsNotFoundAndStays() async throws {
        let rig = Rig(afterRegister: Self.probe(.notFound))
        try await rig.attachAndSettle()
        #expect(rig.model.assessment?.state == .notRegistered)
        await rig.model.turnOn()
        #expect(rig.model.assessment?.state == .notFound)
        await rig.model.refresh()
        #expect(rig.model.assessment?.state == .notFound)
        // A probe that is neither notFound nor requiresApproval ends the memory.
        rig.controller.setStatus(Self.probe(.notRegistered))
        await rig.model.refresh()
        #expect(rig.model.justRegistered == false)
        rig.controller.setStatus(Self.probe(.notFound))
        await rig.model.refresh()
        #expect(rig.model.assessment?.state == .notRegistered)
    }

    @Test func startRelaunchesAndWaitsForTheDaemonWhenEnabled() async throws {
        let rig = Rig(
            status: Self.probe(.enabled), afterRegister: Self.probe(.enabled),
            record: LaunchRecordStatus(exists: true, lastLaunchedAt: Self.start))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.start() }
        await rig.sleeper.waitUntilSleeping(2)  // the poll and the wait
        #expect(rig.controller.calls.contains(.relaunch))
        #expect(rig.controller.calls.contains(.register) == false)
        #expect(rig.model.isStarting)
        #expect(rig.model.isWorking)
        #expect(rig.retries == 1)
        #expect(rig.sleeper.durations.contains(AgentTiming.startWait))
        rig.model.presenceDidChange(.refreshing)
        #expect(rig.model.isStarting)
        rig.model.presenceDidChange(.connected(health: .ready))
        await action.value
        #expect(rig.model.isStarting == false)
        #expect(rig.model.isWorking == false)
        #expect(rig.model.lastError == nil)
    }

    @Test func turnOnWaitsForTheDaemonTheSameWay() async throws {
        let rig = Rig(afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.turnOn() }
        while rig.model.isStarting == false { await Task.yield() }
        #expect(rig.retries == 1)
        #expect(rig.controller.count(of: .openApprovalSettings) == 0)
        rig.model.presenceDidChange(.connected(health: .ready))
        await action.value
        #expect(rig.model.isWorking == false)
    }

    @Test func noHandshakeBeforeTheLimitIsDaemonDidNotStart() async throws {
        let rig = Rig(status: Self.probe(.enabled), afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.start() }
        while rig.model.isStarting == false { await Task.yield() }
        await rig.sleeper.waitUntilSleeping(2)  // the poll and the wait
        rig.sleeper.fire()
        await action.value
        #expect(rig.model.lastError == .daemonDidNotStart(seconds: 30))
        #expect(rig.model.isStarting == false)
        #expect(rig.model.isWorking == false)
    }

    @Test func theLimitDoesNotApplyAfterAHandshake() async throws {
        let rig = Rig(status: Self.probe(.enabled), afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.start() }
        while rig.model.isStarting == false { await Task.yield() }
        await rig.sleeper.waitUntilSleeping(2)  // the poll and the wait
        rig.model.presenceDidChange(.refreshing)
        rig.sleeper.fire()
        for _ in 0..<20 { await Task.yield() }
        #expect(rig.model.isStarting)
        #expect(rig.model.lastError == nil)
        rig.model.presenceDidChange(.connected(health: .ready))
        await action.value
        #expect(rig.model.lastError == nil)
        #expect(rig.model.isStarting == false)
    }

    @Test func aDaemonThatAnsweredAndWentAwayEndsTheWaitWithoutAnError() async throws {
        let rig = Rig(status: Self.probe(.enabled), afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.start() }
        while rig.model.isStarting == false { await Task.yield() }
        rig.model.presenceDidChange(.refreshing)
        rig.model.presenceDidChange(.unreachable(lastKnownAt: nil))
        await action.value
        #expect(rig.model.lastError == nil)
        #expect(rig.model.isStarting == false)
    }

    @Test func aDaemonThatIsAlreadyConnectedEndsTheWaitAtOnce() async throws {
        let rig = Rig(afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.connected(health: .ready))
        await rig.model.turnOn()
        #expect(rig.model.isStarting == false)
        #expect(rig.model.isWorking == false)
    }

    @Test func approvalStillDueAfterTurnOnShowsTheStateAndNoError() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        rig.controller.setAfterRegister(
            .failure(.os(op: .register, kind: .launchDenied, domain: "d", code: 1)))
        rig.controller.setStatus(Self.probe(.requiresApproval))
        await rig.model.turnOn()
        #expect(rig.model.assessment?.state == .requiresApproval)
        #expect(rig.model.justRegistered == false)
        #expect(rig.model.lastError == nil)
    }

    @Test func anOSErrorBecomesTheLastErrorAndTheStatusIsReadAgain() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        let error = AgentActionError.os(
            op: .register, kind: .invalidSignature, domain: "d", code: 3)
        rig.controller.setAfterRegister(.failure(error))
        await rig.model.turnOn()
        #expect(rig.model.lastError == error)
        #expect(rig.controller.calls == [.status, .register, .status])
        // The next action clears it.
        rig.controller.setAfterRegister(Self.probe(.requiresApproval))
        await rig.model.turnOn()
        #expect(rig.model.lastError == nil)
    }

    @Test func aTimedOutRegisterIsTheLastError() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        rig.controller.setAfterRegister(.failure(.timedOut(op: .register)))
        await rig.model.turnOn()
        #expect(rig.model.lastError == .timedOut(op: .register))
    }

    @Test func aRegistrationTheOSDidNotKeepIsAnError() async throws {
        let rig = Rig(afterRegister: Self.probe(.notRegistered))
        try await rig.attachAndSettle()
        await rig.model.turnOn()
        #expect(rig.model.lastError == .registrationNotKept)
    }

    @Test func alreadyRegisteredCountsAsDoneAndReadsTheStatusAgain() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        rig.controller.setAfterRegister(
            .failure(.os(op: .register, kind: .alreadyRegistered, domain: "d", code: 1)))
        rig.controller.setStatus(Self.probe(.requiresApproval))
        await rig.model.turnOn()
        #expect(rig.model.lastError == nil)
        #expect(rig.model.justRegistered)
        #expect(rig.model.assessment?.state == .requiresApproval)
    }

    @Test func openLoginItemsOnlyOpensSettings() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        rig.model.openLoginItems()
        #expect(rig.controller.calls == [.status, .openApprovalSettings])
    }

    @Test func anActionWaitsForARefreshThatRunsAndItsResultWins() async throws {
        let rig = Rig(afterRegister: Self.probe(.requiresApproval))
        rig.controller.holdNextCall()  // the launch refresh's status call
        rig.attach(.connecting(slow: false))
        await rig.controller.waitUntilHolding()
        let action = Task { await rig.model.turnOn() }
        for _ in 0..<20 { await Task.yield() }
        #expect(rig.model.isWorking)
        #expect(rig.controller.calls == [.status])
        rig.controller.release()
        await action.value
        #expect(rig.controller.calls.prefix(2) == [.status, .register])
        #expect(rig.model.assessment?.state == .requiresApproval)
    }

    @Test func aSecondActionWhileOneRunsDoesNothing() async throws {
        let rig = Rig(afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let first = Task { await rig.model.turnOn() }
        while rig.model.isStarting == false { await Task.yield() }
        await rig.model.turnOn()
        await rig.model.start()
        #expect(rig.controller.count(of: .register) == 1)
        #expect(rig.controller.count(of: .relaunch) == 0)
        rig.model.presenceDidChange(.connected(health: .ready))
        await first.value
    }

    // MARK: - State becomes enabled on its own

    @Test func approvalGivenInSystemSettingsStartsTheWaitWithoutRegistering() async throws {
        let rig = Rig(status: Self.probe(.requiresApproval))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let callsBefore = rig.controller.calls
        rig.controller.setStatus(Self.probe(.enabled))
        await rig.model.refresh()
        #expect(rig.model.assessment?.state == .enabled)
        #expect(rig.model.isStarting)
        #expect(rig.model.isWorking)
        #expect(rig.retries == 1)
        #expect(rig.controller.calls == callsBefore + [.status])
        while rig.sleeper.durations.contains(AgentTiming.startWait) == false {
            await Task.yield()
        }
        rig.model.presenceDidChange(.connected(health: .ready))
        #expect(rig.model.isStarting == false)
        #expect(rig.model.isWorking == false)
    }

    @Test func enabledStaysEnabledWithoutAnotherWait() async throws {
        let rig = Rig(status: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        await rig.model.refresh()
        #expect(rig.model.isStarting == false)
        #expect(rig.retries == 0)
    }

    // MARK: - Reports

    @Test func refreshUsesTheInjectedClock() async throws {
        let rig = Rig(status: Self.probe(.enabled))
        try await rig.attachAndSettle()
        rig.clock = Self.start.addingTimeInterval(42)
        await rig.model.refresh()
        #expect(rig.model.lastCheckedAt == Self.start.addingTimeInterval(42))
    }

    // MARK: - Failures are reported (step 12a)

    @Test func aWaitThatReachesTheLimitReportsDaemonDidNotStartOnce() async throws {
        let rig = Rig(status: Self.probe(.enabled), afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.start() }
        while rig.model.isStarting == false { await Task.yield() }
        await rig.sleeper.waitUntilSleeping(2)  // the poll and the wait
        #expect(rig.model.lastError == nil)
        rig.sleeper.fire()
        await action.value
        #expect(rig.model.lastError == .daemonDidNotStart(seconds: 30))
    }

    @Test func anOSErrorIsStoredAfterTheAction() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        let error = AgentActionError.os(
            op: .register, kind: .invalidSignature, domain: "d", code: 3)
        rig.controller.setAfterRegister(.failure(error))
        await rig.model.turnOn()
        #expect(rig.model.lastError == error)
        #expect(rig.model.lastError == error)
    }

    @Test func aTimedOutRegisterIsReported() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        rig.controller.setAfterRegister(.failure(.timedOut(op: .register)))
        await rig.model.turnOn()
        #expect(rig.model.lastError == .timedOut(op: .register))
    }

    @Test func aRegistrationTheOSDidNotKeepIsReported() async throws {
        let rig = Rig(afterRegister: Self.probe(.notRegistered))
        try await rig.attachAndSettle()
        await rig.model.turnOn()
        #expect(rig.model.lastError == .registrationNotKept)
    }

    @Test func aRefreshThatFailsReportsNothingThroughDidFail() async throws {
        let rig = Rig(status: .failure(.timedOut(op: .status)))
        try await rig.attachAndSettle()
        await rig.model.refresh()
        #expect(rig.model.assessment?.state == .unavailable)
        #expect(rig.model.lastError == nil)
        #expect(rig.model.lastError == nil)
    }

    @Test func clearingTheLastErrorAtTheStartOfTheNextActionReportsNothing() async throws {
        let rig = Rig()
        try await rig.attachAndSettle()
        rig.controller.setAfterRegister(.failure(.timedOut(op: .register)))
        await rig.model.turnOn()
        #expect(rig.model.lastError == .timedOut(op: .register))
        rig.controller.setAfterRegister(Self.probe(.requiresApproval))
        await rig.model.turnOn()
        #expect(rig.model.lastError == nil)

    }

    // MARK: - The wait ends when launchd cannot start the job (step 12a)

    /// Runs Turn On into the wait for the daemon, then changes the status the next refresh
    /// reads to `next` (with a launch record) and refreshes, as `didBecomeActive()` does.
    private func waitThenRefresh(
        into next: Result<AgentProbe, AgentActionError>, record: Bool = true
    ) async throws -> Rig {
        let rig = Rig(afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.turnOn() }
        while rig.model.isStarting == false { await Task.yield() }
        await rig.sleeper.waitUntilSleeping(2)  // the poll and the wait
        rig.record.set(
            record ? LaunchRecordStatus(exists: true, lastLaunchedAt: Self.start) : .missing)
        rig.controller.setStatus(next)
        await rig.model.refresh()
        await action.value
        return rig
    }

    @Test func aProbeThatNeedsApprovalEndsTheWaitWithoutAnError() async throws {
        let rig = try await waitThenRefresh(into: Self.probe(.requiresApproval), record: false)
        #expect(rig.model.assessment?.state == .requiresApproval)
        #expect(rig.model.isStarting == false)
        #expect(rig.model.isWorking == false)
        #expect(rig.model.lastError == nil)
        #expect(rig.model.lastError == nil)
        // The wait's own sleep was cancelled; only the poll still waits.
        #expect(rig.sleeper.waiting == 1)
    }

    @Test func aProbeThatSaysDisabledEndsTheWaitWithoutAnError() async throws {
        let rig = try await waitThenRefresh(into: Self.probe(.requiresApproval))
        #expect(rig.model.assessment?.state == .disabled)
        #expect(rig.model.isStarting == false)
        #expect(rig.model.isWorking == false)
        #expect(rig.model.lastError == nil)
        #expect(rig.sleeper.waiting == 1)
    }

    @Test func aProbeThatSaysNotRegisteredEndsTheWaitWithoutAnError() async throws {
        let rig = try await waitThenRefresh(into: Self.probe(.notFound), record: false)
        // `justRegistered` is false after an `enabled` result, so `notFound` reads as off.
        #expect(rig.model.assessment?.state == .notRegistered)
        #expect(rig.model.isStarting == false)
        #expect(rig.model.isWorking == false)
        #expect(rig.model.lastError == nil)
    }

    @Test func aStatusThatTimesOutDuringTheWaitDoesNotEndItAndTheLimitStillApplies()
        async throws
    {
        let rig = Rig(afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.turnOn() }
        while rig.model.isStarting == false { await Task.yield() }
        await rig.sleeper.waitUntilSleeping(2)
        rig.controller.setStatus(.failure(.timedOut(op: .status)))
        await rig.model.refresh()
        #expect(rig.model.assessment?.state == .unavailable)
        #expect(rig.model.isStarting)
        #expect(rig.model.isWorking)
        rig.sleeper.fire()
        await action.value
        #expect(rig.model.lastError == .daemonDidNotStart(seconds: 30))
        #expect(rig.model.lastError == .daemonDidNotStart(seconds: 30))
    }

    @Test func aStateThatBecomesEnabledAfterTheWaitEndedStartsANewWait() async throws {
        let rig = try await waitThenRefresh(into: Self.probe(.requiresApproval))
        #expect(rig.model.isStarting == false)
        #expect(rig.retries == 1)
        rig.controller.setStatus(Self.probe(.enabled))
        await rig.model.refresh()
        #expect(rig.model.isStarting)
        #expect(rig.retries == 2)
    }

    // MARK: - Polling

    @Test func pollingWaitsBeforeItsFirstStatusAndStopsWhenThePresenceLeavesUnreachable()
        async throws
    {
        let rig = Rig(initialProbe: Self.probe(.enabled))
        rig.attach(.unreachable(lastKnownAt: nil))
        await rig.sleeper.waitUntilSleeping()
        #expect(rig.sleeper.durations == [AgentTiming.pollInterval])
        #expect(rig.controller.calls.isEmpty)
        rig.sleeper.fire()
        try await waitUntil { rig.controller.count(of: .status) == 1 }
        await rig.sleeper.waitUntilSleeping()
        rig.model.presenceDidChange(.connected(health: .ready))
        try await waitUntil { rig.sleeper.waiting == 0 }
    }

    @Test func pausingStopsThePollingAndResumingStartsItAgain() async throws {
        let rig = Rig(initialProbe: Self.probe(.enabled))
        rig.attach(.unreachable(lastKnownAt: nil))
        await rig.sleeper.waitUntilSleeping()
        rig.model.pausePolling()
        try await waitUntil { rig.sleeper.waiting == 0 }
        rig.model.resumePolling()
        await rig.sleeper.waitUntilSleeping()
        #expect(rig.sleeper.durations == [AgentTiming.pollInterval, AgentTiming.pollInterval])
    }

    @Test func pausingDoesNotStopTheWaitForTheDaemon() async throws {
        let rig = Rig(status: Self.probe(.enabled), afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.start() }
        while rig.model.isStarting == false { await Task.yield() }
        rig.model.pausePolling()
        for _ in 0..<20 { await Task.yield() }
        #expect(rig.model.isStarting)
        rig.model.presenceDidChange(.connected(health: .ready))
        await action.value
    }

    @Test func theTransitionToUnreachableStartsARefresh() async throws {
        let rig = Rig(status: Self.probe(.enabled))
        rig.attach(.connected(health: .ready))
        try await waitUntil { rig.model.assessment != nil }
        let before = rig.controller.count(of: .status)
        rig.model.presenceDidChange(.unreachable(lastKnownAt: nil))
        try await waitUntil { rig.controller.count(of: .status) == before + 1 }
    }

    // MARK: - Stop

    @Test func stopCancelsThePollingTheWaitAndTheRefresh() async throws {
        let rig = Rig(status: Self.probe(.enabled), afterRegister: Self.probe(.enabled))
        try await rig.attachAndSettle(.unreachable(lastKnownAt: nil))
        let action = Task { await rig.model.start() }
        while rig.model.isStarting == false { await Task.yield() }
        rig.model.stop()
        await action.value
        try await waitUntil { rig.sleeper.waiting == 0 }
        #expect(rig.model.isStarting == false)
        #expect(rig.model.isWorking == false)
        rig.model.presenceDidChange(.unreachable(lastKnownAt: nil))
        for _ in 0..<20 { await Task.yield() }
        #expect(rig.sleeper.waiting == 0)
    }

    // MARK: - The fake

    @Test func theFakeAnswersAfterRegisterToStatusOnceRegistered() async throws {
        let fake = FakeBackgroundAgentController()
        #expect(try await fake.status() == .registration(.notFound, bundle: Self.bundle))
        #expect(try await fake.register() == .registration(.requiresApproval, bundle: Self.bundle))
        #expect(try await fake.status() == .registration(.requiresApproval, bundle: Self.bundle))
        #expect(try await fake.unregister() == .registration(.notRegistered, bundle: Self.bundle))
        #expect(try await fake.status() == .registration(.notRegistered, bundle: Self.bundle))
        #expect(try await fake.relaunch() == .registration(.requiresApproval, bundle: Self.bundle))
        #expect(fake.calls == [.status, .register, .status, .unregister, .status, .relaunch])
    }
}
