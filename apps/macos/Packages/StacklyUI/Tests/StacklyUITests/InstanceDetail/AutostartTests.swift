import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// The Autostart row of the Summary tab (spec 0031, AC-16, AC-19): the words of every state
/// and the change of the policy through the detail service.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct AutostartTests {
    private func text(
        _ policy: StartupPolicyKind, _ session: AutostartSession,
        desired: InstanceDesired = .running,
        rotation: SessionRotationKind? = .new, agent: BackgroundAgentState? = .enabled,
        rootOverride: Bool = false
    ) -> String? {
        AutostartText.note(
            autostart: InstanceAutostart(policy: policy, session: session), desired: desired,
            context: AutostartContext(
                rotation: rotation, agent: agent, rootOverride: rootOverride))
    }

    @Test func aManualInstanceHasNoSecondaryText() {
        #expect(text(.manual, .none) == nil)
        #expect(text(.manual, .none, agent: .disabled) == nil)
    }

    @Test func aStoppedInstanceThatStartsAtLoginSaysItWaitsForTheNextLogin() {
        #expect(
            text(.atLogin, .suppressed, desired: .stopped)
                == "Stopped for this session. Starts at your next login.")
    }

    @Test func anInstanceWithoutALeaseInASessionThatIsNotNewSaysItStartsAtTheNextLogin() {
        #expect(text(.atLogin, .none, rotation: .continued) == "Starts at your next login.")
        #expect(text(.atLogin, .none, rotation: .first) == "Starts at your next login.")
        #expect(text(.atLogin, .none, rotation: nil) == "Starts at your next login.")
        #expect(text(.atLogin, .none, rotation: .new) == nil)
        #expect(text(.atLogin, .active) == nil)
    }

    @Test func aBackgroundAgentThatIsNotOnKeepsTheInstanceFromStartingAtLogin() {
        #expect(
            text(.atLogin, .none, agent: .notRegistered)
                == "Won’t start at login: the background agent is off.")
        #expect(
            text(.atLogin, .active, agent: .disabled)
                == "Won’t start at login: the background agent is off.")
        #expect(
            text(.atLogin, .none, agent: .notFound)
                == "Won’t start at login: the background agent is off.")
        #expect(
            text(.atLogin, .none, agent: .requiresApproval)
                == "Won’t start at login: the background agent needs approval in Login Items.")
    }

    @Test func theAgentOnlyCountsInASessionOnTheDefaultRoot() {
        #expect(
            text(.atLogin, .none, rotation: nil, agent: .disabled) == "Starts at your next login.")
        #expect(
            text(.atLogin, .none, agent: .disabled, rootOverride: true) == nil)
        #expect(text(.atLogin, .none, agent: .unavailable) == nil)
    }

    @Test func anOlderDaemonReadsAsManualWithNoLease() throws {
        let full = try DetailFixture.triple("postgresql").instance
        var object = try #require(
            JSONSerialization.jsonObject(with: StacklyJSON.encoder().encode(full))
                as? [String: Any])
        object["autostart"] = nil
        let summary = try StacklyJSON.decoder().decode(
            InstanceSummary.self, from: JSONSerialization.data(withJSONObject: object))

        #expect(summary.startup == InstanceAutostart(policy: .manual, session: .none))
    }

    @Test func changingTheToggleSendsTheMatchingPolicyOnce() async throws {
        let item = try DetailFixture.triple("postgresql")
        let script = DetailScript(detail: item.detail, usage: item.dataUsage)
        let sent = Mutex<[StartupPolicyKind]>([])
        var service = script.service
        service.setStartup = { _, startup in
            sent.withLock { $0.append(startup) }
            return .rejected(.refusal("supervisor.instance_retained"))
        }
        let model = InstanceDetailModel(instanceId: item.detail.instanceId, service: service)

        let on = await model.setAutostart(true)
        let off = await model.setAutostart(false)

        #expect(sent.withLock { $0 } == [.atLogin, .manual])
        #expect(on == .rejected(.refusal("supervisor.instance_retained")))
        #expect(off == on)
        #expect(model.isChangingAutostart == false)
    }

    @Test func aDetailServiceWithoutADaemonCannotSend() async throws {
        let item = try DetailFixture.triple("postgresql")
        let script = DetailScript(detail: item.detail, usage: item.dataUsage)
        let model = InstanceDetailModel(
            instanceId: item.detail.instanceId, service: script.service)

        let outcome = await model.setAutostart(true)

        guard case .notSent = outcome else {
            Issue.record("expected notSent, got \(outcome)")
            return
        }
        #expect(model.autostartContext == .unknown)
    }
}
