import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The override of the other actions in the app (spec 0047, AC-23, AC-24): the Ports sheet
/// of an instance an environment's lease holds, the Stop All popover, and Turn Off.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct EnvironmentOverrideTests {
    private let sleeper = ManualSleep()
    private let source = InstanceEditSource(id: ConfigScript.instance, name: "orders")

    private static let busy = ErrorBody(
        code: "resource.busy", category: .conflict, message: "busy", retryable: false,
        resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
        items: nil)

    private static func holder(_ name: String) -> InstanceEnvironment {
        InstanceEnvironment(
            environmentId: UUID(), name: name, alias: "db", ownership: .shared, leaseActive: true)
    }

    private func judged(_ script: ConfigScript, leases: [InstanceEnvironment]?) async
        -> EndpointsModel
    {
        script.judgeEndpoints(
            .success(
                EndpointValidation(
                    valid: true, problems: [],
                    changes: [EndpointChange(name: "main", old: .fixed(5555), new: .fixed(5600))],
                    impact: .restart, leases: leases)))
        let sleeper = sleeper
        let model = EndpointsModel(
            source: source, service: script.service,
            sleep: { try await sleeper.sleep(for: $0) })
        await model.load()
        model.setPortText("5600", of: "main")
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        while model.isValidating { await Task.yield() }
        return model
    }

    @Test(
        .evidence(
            covers: ["REQ-028"],
            expected:
                "When the dry run names an environment whose lease holds the instance, the Ports sheet shows the caution and Change Anyway, which sends overrideLeases true"
        ))
    func aLeasedInstanceAsksChangeAnywayAndSendsTheOverride() async throws {
        let script = try ConfigScript(view: ConfigScript.fakeView())
        let model = await judged(script, leases: [Self.holder("shop"), Self.holder("blog")])
        #expect(
            model.leaseWarning
                == "shop and blog uses this instance. Changing its ports can break that environment until you update what connects to it."
        )
        #expect(model.primaryTitle == "Change Anyway")
        #expect(model.disabledReason == nil)
        script.answer(.rejected(Self.busy), ending: nil)
        _ = await model.apply(context: InstanceActionContext(report: { _ in }))
        guard case .instanceChangeEndpoints(let params)? = script.sent.first?.call else {
            Issue.record("expected instance.change_endpoints")
            return
        }
        #expect(params.dryRun == false)
        #expect(params.overrideLeases == true)
    }

    @Test func withoutALeaseTheSheetSendsNoOverride() async throws {
        let script = try ConfigScript(view: ConfigScript.fakeView())
        let model = await judged(script, leases: [])
        #expect(model.leaseWarning == nil)
        #expect(model.primaryTitle == "Change Ports")
        script.answer(.rejected(Self.busy), ending: nil)
        _ = await model.apply(context: InstanceActionContext(report: { _ in }))
        guard case .instanceChangeEndpoints(let params)? = script.sent.first?.call else {
            Issue.record("expected instance.change_endpoints")
            return
        }
        #expect(params.overrideLeases == nil)
    }

    // MARK: - Stop All and Turn Off

    private static func item(
        _ name: String, demand: [DemandSourceKind], environments: [String] = []
    ) -> StopAllItem {
        StopAllItem(
            instanceId: UUID(), name: name, lifecycle: .running, demand: demand,
            autostart: InstanceAutostart(policy: .manual, session: .none),
            environments: environments.map(holder))
    }

    private static func preview(_ items: [StopAllItem], affected: [String]) -> StopAllPreview {
        StopAllPreview(
            items: items, alreadyStopping: [], skipped: [], atLoginCount: 0,
            affectedEnvironments: affected.map {
                AffectedEnvironment(environmentId: UUID(), name: $0, instanceCount: 1)
            })
    }

    @Test func aStopAllRowNamesTheEnvironmentsThatUseIt() {
        #expect(
            StopAllCopy.demand(
                of: Self.item(
                    "pg", demand: [.environment, .manual], environments: ["shop", "blog"]))
                == "Used by shop, blog, Started by you")
        #expect(
            StopAllCopy.demand(of: Self.item("pg", demand: [.environment]))
                == "Used by an environment")
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected:
                "The Stop All preview says which environments will no longer be complete and that their leases stay"
        ))
    func theStopAllFooterNamesTheAffectedEnvironments() {
        #expect(StopAllCopy.affected(Self.preview([], affected: [])) == nil)
        #expect(
            StopAllCopy.affected(Self.preview([], affected: ["shop"]))
                == "shop will no longer be complete. Its lease stays.")
        #expect(
            StopAllCopy.affected(Self.preview([], affected: ["blog", "shop"]))
                == "2 environments will no longer be complete: blog, shop. Their leases stay.")
    }

    @Test func turnOffListsTheLeasedInstancesAndIsNotOffForALease() {
        let plan = TurnOffPlan(
            preview: Self.preview(
                [
                    Self.item("cache", demand: [.manual]),
                    Self.item("shop-db", demand: [.environment], environments: ["shop"]),
                ], affected: ["shop"]),
            atLoginCount: 0)
        #expect(plan.canTurnOff)
        #expect(
            plan.message.hasSuffix(
                "These stop too: shop-db · Used by shop. Environments keep their leases. Bring them up again after you turn Stackly back on."
            ))
    }
}
