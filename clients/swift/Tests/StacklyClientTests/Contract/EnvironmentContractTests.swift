import Foundation
import StacklyClient
import Testing

/// The environment methods of protocol 4.2 from the shared fixtures (spec
/// 0044, *API surface*): the plan, validation, and detail DTOs, and values
/// a newer daemon sends, which read as `.unrecognized`.
struct EnvironmentContractTests {
    static func plan(_ file: String) throws -> EnvPlan {
        try StacklyJSON.decoder().decode(EnvPlan.self, from: Fixture.data("env/\(file)"))
    }

    @Test func anApplicablePlanHasItsDigestStepsAndSecretStatus() throws {
        let plan = try Self.plan("plan-applicable.json")
        #expect(plan.applicable)
        #expect(plan.planDigest != nil)
        #expect(plan.items?.first?.action == .create)
        #expect(plan.items?.first?.steps == [.installRuntime, .createInstance, .startInstance])
        #expect(plan.items?.first?.secrets?.first?.status == .unregistered)
        #expect(plan.items?.last?.action == .keep)
    }

    @Test func aBlockedPlanCarriesErrorBodies() throws {
        let plan = try Self.plan("plan-blocked.json")
        #expect(!plan.applicable)
        #expect(
            plan.blockers?.map(\.error.code) == [
                "supervisor.endpoint_conflict", "env.instance_blocked",
            ])
    }

    @Test func aDetailReadsItsStatusAndNotReadyReason() throws {
        let detail = try StacklyJSON.decoder().decode(
            EnvironmentDetail.self, from: Fixture.data("env/detail-partially-ready.json"))
        #expect(detail.summary.status == .partiallyReady)
        #expect(detail.services?.first?.notReady?.reason == .notCreated)
        #expect(detail.manifestState == .changed)
    }

    @Test func aNewerStatusReadsAsUnrecognized() throws {
        guard
            case .response(let response) = try Fixture.ipc(
                ServerFrame.self, "frame-response-env-list-unknown-values.json"),
            case .envList(let list) = response.reply
        else {
            Issue.record("expected an env.list reply")
            return
        }
        #expect(list.items?.first?.status == .unrecognized)
    }
}
