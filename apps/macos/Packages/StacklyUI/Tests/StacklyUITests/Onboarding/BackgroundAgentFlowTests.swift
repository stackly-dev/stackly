import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The card's buttons call the right methods of the agent model and nothing else, and the
/// card follows the model through a Turn On (spec 0016, AC-5).
@Suite(.timeLimit(.minutes(1)))
struct BackgroundAgentFlowTests {
    typealias Rig = DaemonModelAgentTests.Rig

    private static func card(_ rig: Rig, after seconds: TimeInterval = 0)
        -> BackgroundAgentStepContent?
    {
        rig.model.agentBannerInput.map {
            BackgroundAgentStepContent.make(
                $0, footer: rig.model.presentation.footer,
                now: DaemonModelTests.launch.addingTimeInterval(5 + seconds),
                lastError: rig.agent?.lastError, isWorking: rig.agent?.isWorking ?? false)
        }
    }

    @Test func aFreshInstallShowsTurnOnWithoutAnyCallButStatus() async throws {
        let rig = Rig(initialProbe: DaemonModelAgentTests.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        let content = try #require(Self.card(rig))
        #expect(content.title == "Background service is off")
        #expect(content.action == .turnOn)
        #expect(rig.controller.calls.allSatisfy { $0 == .status })
    }

    @Test func turnOnCallsRegisterOnlyAndTheCardWaitsForTheDaemon() async throws {
        let rig = Rig(
            afterRegister: DaemonModelAgentTests.fresh(.enabled),
            initialProbe: DaemonModelAgentTests.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        let action = Task { await rig.agent?.run(.turnOn) }
        while rig.agent?.isStarting != true { await Task.yield() }
        let waiting = try #require(Self.card(rig))
        #expect(waiting.detail == "stacklyd is starting…")
        #expect(waiting.action == nil)
        #expect(waiting.isWorking)
        await rig.waitForSleep(AgentTiming.startWait)
        rig.sleeper.fire()
        await action.value
        #expect(rig.controller.count(of: .register) == 1)
        #expect(rig.controller.count(of: .unregister) == 0)
        #expect(rig.controller.count(of: .relaunch) == 0)
        let after = try #require(Self.card(rig, after: 40))
        #expect(after.lastError == "stacklyd didn’t start within 30 seconds.")
        #expect(after.action == .start)
        #expect(after.isWorking == false)
    }

    @Test func approvalEndsTheWaitWithoutAnErrorAndOffersLoginItems() async throws {
        let rig = Rig(
            afterRegister: DaemonModelAgentTests.fresh(.requiresApproval),
            initialProbe: DaemonModelAgentTests.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        await rig.agent?.run(.turnOn)
        let content = try #require(Self.card(rig))
        #expect(content.title == "Waiting for approval")
        #expect(content.action == .openLoginItems)
        #expect(content.lastError == nil)
        #expect(content.isWorking == false)
    }

    @Test func aTurnOnThatEndsNotFoundHasNoActionAndACriticalRow() async throws {
        let rig = Rig(
            afterRegister: DaemonModelAgentTests.fresh(.notFound),
            initialProbe: DaemonModelAgentTests.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        await rig.agent?.run(.turnOn)
        let content = try #require(Self.card(rig))
        #expect(content.title == "Stackly can’t find its background service")
        #expect(content.action == nil)
    }

    @Test func openLoginItemsOnlyOpensTheSettings() async throws {
        let rig = Rig(
            status: DaemonModelAgentTests.fresh(.requiresApproval),
            initialProbe: DaemonModelAgentTests.fresh(.requiresApproval))
        try await rig.push(DaemonModelTests.unreachable())
        await rig.agent?.run(.openLoginItems)
        #expect(rig.controller.calls.filter { $0 != .status } == [.openApprovalSettings])
    }

    @Test func checkAgainOnlyReadsTheStatus() async throws {
        let rig = Rig(initialProbe: DaemonModelAgentTests.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        await rig.agent?.run(.checkAgain)
        #expect(rig.controller.calls.allSatisfy { $0 == .status })
        #expect(rig.controller.count(of: .status) >= 1)
    }

    @Test func startRelaunchesOnlyWhenTheCardOffersIt() async throws {
        let rig = Rig(
            status: DaemonModelAgentTests.fresh(.enabled),
            afterRegister: DaemonModelAgentTests.fresh(.enabled),
            record: LaunchRecordStatus(exists: true, lastLaunchedAt: nil),
            initialProbe: DaemonModelAgentTests.fresh(.enabled))
        try await rig.push(DaemonModelTests.unreachable())
        let early = try #require(Self.card(rig, after: 0))
        #expect(early.action == nil)
        let late = try #require(Self.card(rig, after: 20))
        #expect(late.action == .start)
        let action = Task { await rig.agent?.run(.start) }
        while rig.agent?.isStarting != true { await Task.yield() }
        let waiting = try #require(Self.card(rig, after: 60))
        #expect(waiting.action == nil)
        await rig.waitForSleep(AgentTiming.startWait)
        rig.sleeper.fire()
        await action.value
        #expect(rig.controller.count(of: .relaunch) == 1)
    }

    @Test func aStateThatBecomesEnabledOutsideAnActionStartsTheWaitWithoutStartOffered()
        async throws
    {
        let rig = Rig(initialProbe: DaemonModelAgentTests.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        rig.controller.setStatus(DaemonModelAgentTests.fresh(.enabled))
        await rig.agent?.refresh()
        for _ in 0..<20 { await Task.yield() }
        let content = try #require(Self.card(rig, after: 60))
        #expect(content.detail == "stacklyd is starting…")
        #expect(content.action == nil)
        #expect(content.isWorking)
    }

    @Test func aWaitEndsWhenTheStateBecomesRequiresApprovalOrDisabled() async throws {
        let rig = Rig(initialProbe: DaemonModelAgentTests.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        rig.controller.setStatus(DaemonModelAgentTests.fresh(.enabled))
        await rig.agent?.refresh()
        for _ in 0..<20 { await Task.yield() }
        rig.controller.setStatus(DaemonModelAgentTests.fresh(.requiresApproval))
        await rig.agent?.refresh()
        for _ in 0..<20 { await Task.yield() }
        let content = try #require(Self.card(rig))
        #expect(content.title == "Waiting for approval")
        #expect(content.isWorking == false)
        #expect(content.action == .openLoginItems)
    }

    @Test func theLastErrorOfAFailedTurnOnIsAnnouncedByTheModelOnce() async throws {
        let rig = Rig(initialProbe: DaemonModelAgentTests.fresh())
        try await rig.push(DaemonModelTests.unreachable())
        let error = AgentActionError.os(
            op: .register, kind: .invalidSignature, domain: "d", code: 3)
        rig.controller.setAfterRegister(.failure(error))
        await rig.agent?.run(.turnOn)
        let content = try #require(Self.card(rig))
        #expect(content.lastError == "macOS rejected the signature of this build.")
        #expect(rig.announced.filter { $0 == error.message }.count == 1)
    }
}
