import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// Up… and Bring Up of the plan sheet (spec 0046, AC-19, AC-21).
@MainActor
struct EnvironmentUpPresenterTests {
    let rig = PageRig(environments: FixtureEnvironments.summaries)
    let source = EnvironmentPlanPresenter.Source.review(
        environment: FixtureEnvironments.erpID, name: "erp-development")

    private func presenter() -> EnvironmentPlanPresenter {
        EnvironmentPlanPresenter(
            store: rig.environments, picker: FixtureFilePicker(path: nil), newKey: { "key-1" })
    }

    private static func error(_ code: String, items: [ErrorItem]? = nil) -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: code, retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: items)
    }

    @Test(
        .evidence(
            covers: ["REQ-021", "NFR-012"],
            expected:
                "Bring Up sends env.up for the registered manifest with the planDigest the sheet showed, then closes the sheet"
        ))
    func bringUpSendsTheDigestTheSheetShowed() async throws {
        rig.script.plan = FixtureEnvironmentUp.upPlan()
        let presenter = presenter()
        var accepted: EnvUpReply?
        presenter.broughtUp = { accepted = $0 }
        await presenter.read(source)
        let plan = try #require(presenter.plan)
        #expect(EnvironmentPlanPresenter.upBlockedReason(plan) == nil)
        #expect(rig.script.environmentSent.isEmpty)

        await presenter.bringUp()
        guard case .envUp(let params) = try #require(rig.script.environmentSent.first) else {
            Issue.record("expected env.up")
            return
        }
        #expect(params.source == .registered(FixtureEnvironments.erpID))
        #expect(params.expectedPlanDigest == plan.planDigest)
        #expect(presenter.phase == .idle)
        #expect(accepted?.operation.operation.kind == .environmentUp)
    }

    @Test func aPlanThatIsNotApplicableCantBeBroughtUp() async throws {
        let presenter = presenter()
        await presenter.read(source)
        let plan = try #require(presenter.plan)
        #expect(
            EnvironmentPlanPresenter.upBlockedReason(plan) == "Resolve the blockers above first.")
        #expect(
            EnvironmentPlanSheet.bringUpAction(presenter).disabledReason
                == "Resolve the blockers above first.")
        await presenter.bringUp()
        #expect(rig.script.environmentSent.isEmpty)
    }

    @Test func aChangedPlanIsReadAgainWithANote() async throws {
        rig.script.plan = FixtureEnvironmentUp.upPlan()
        rig.script.environmentOutcome = .rejected(Self.error("env.plan_changed"))
        let presenter = presenter()
        await presenter.read(source)
        await presenter.bringUp()
        guard case .plan = presenter.phase else {
            Issue.record("expected the plan again, got \(presenter.phase)")
            return
        }
        #expect(presenter.notice == "The plan changed. Review it again.")
        #expect(rig.script.environmentSent.count == 1)
    }

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "secret.needs_secret from env.up opens the secret entry sheet with its references; Continue reads the plan again"
        ))
    func aMissingSecretOpensTheSecretSheetThenReadsThePlanAgain() async throws {
        rig.script.plan = FixtureEnvironmentUp.upPlan()
        let item = ErrorItem(
            kind: "reference",
            fields: [
                ErrorDetail(key: "reference", value: "local:erp/search/adminPassword"),
                ErrorDetail(key: "reason", value: "missing"),
                ErrorDetail(key: "generateAllowed", value: "true"),
            ])
        rig.script.environmentOutcome = .rejected(
            Self.error("secret.needs_secret", items: [item]))
        let presenter = presenter()
        await presenter.read(source)
        await presenter.bringUp()
        #expect(presenter.secretNeeds?.map(\.reference) == ["local:erp/search/adminPassword"])
        #expect(presenter.secretNeeds?.first?.generateAllowed == true)
        guard case .plan = presenter.phase else {
            Issue.record("expected the plan to stay")
            return
        }

        rig.script.environmentOutcome = nil
        await presenter.secretsEntered(saved: true)
        #expect(presenter.secretNeeds == nil)
        #expect(presenter.plan != nil)
        await presenter.bringUp()
        #expect(rig.script.environmentSent.count == 2)
        #expect(presenter.phase == .idle)
    }

    @Test func setOfANeedsSecretLineAsksForThatReference() async throws {
        let presenter = presenter()
        let item = try #require(
            FixtureEnvironmentUp.upPlan().items?.first { $0.action == .create })
        let secret = try #require(item.secrets?.first)
        #expect(PlanItemRow.canSet(secret))
        presenter.setSecret(secret)
        #expect(presenter.secretNeeds?.map(\.reference) == [secret.reference].compactMap { $0 })
        await presenter.secretsEntered(saved: false)
        #expect(presenter.secretNeeds == nil)
        #expect(
            PlanItemRow.canSet(PlanSecret(field: "a", reference: "local:x/y", status: .present))
                == false)
    }

    @Test func aRefusedUpKeepsThePlanAndSaysWhy() async throws {
        rig.script.plan = FixtureEnvironmentUp.upPlan()
        rig.script.environmentOutcome = .rejected(Self.error("env.busy"))
        let presenter = presenter()
        await presenter.read(source)
        await presenter.bringUp()
        guard case .upFailed(_, _, let body) = presenter.phase else {
            Issue.record("expected the refusal")
            return
        }
        #expect(body.code == "env.busy")
        #expect(EnvironmentPlanSheet.bringUpAction(presenter).disabledReason == nil)
    }

    @Test func upIsOffWhileAnOperationRunsOrTheManifestCantBeUsed() {
        let summary = FixtureEnvironments.summaries[1]
        #expect(
            EnvironmentDetailPage.upDisabledReason(
                summary: summary, manifestState: .inSync, problemCount: 0, declaredName: nil)
                == nil)
        #expect(
            EnvironmentDetailPage.upDisabledReason(
                summary: FixtureEnvironmentUp.bringingUpSummary, manifestState: .inSync,
                problemCount: 0, declaredName: nil) == "Already bringing up")
        #expect(
            EnvironmentDetailPage.upDisabledReason(
                summary: summary, manifestState: .invalid, problemCount: 2, declaredName: nil)
                == "The manifest has 2 problems.")
        #expect(
            EnvironmentDetailPage.upDisabledReason(
                summary: summary, manifestState: .nameChanged, problemCount: 0,
                declaredName: "erp") == "This file now declares “erp”.")
        #expect(
            EnvironmentDetailPage.upDisabledReason(
                summary: summary, manifestState: .missing, problemCount: 0, declaredName: nil)
                == "The manifest file can’t be read.")
    }
}
