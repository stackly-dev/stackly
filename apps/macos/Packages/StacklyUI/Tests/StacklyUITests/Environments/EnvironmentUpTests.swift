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

/// The progress and the reasons of an Up (spec 0046, AC-20, AC-22).
@MainActor
struct EnvironmentUpProgressTests {
    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "Each Up phase has its label, tone, and symbol; waiting names the first dependency and how many more"
        ))
    func eachPhaseReadsAsTheCopyTableSays() {
        let table: [(UpPhase, String, Tone, String)] = [
            (.pending, "Queued", .neutral, "clock"),
            (.installing, "Installing…", .informative, "arrow.down.circle"),
            (.creating, "Creating…", .informative, "plus.circle"),
            (.starting, "Starting…", .informative, "play.circle"),
            (.unrecognized, "Working…", .neutral, "ellipsis.circle"),
        ]
        for (phase, label, tone, symbol) in table {
            let appearance = StatusAppearance.upPhase(phase: phase)
            #expect(appearance.label == label)
            #expect(appearance.tone == tone)
            #expect(appearance.symbol == symbol)
        }
        #expect(
            StatusAppearance.upPhase(phase: .waiting, waitingOn: ["postgres"]).label
                == "Waiting for postgres")
        #expect(
            StatusAppearance.upPhase(phase: .waiting, waitingOn: ["postgres", "cache"]).label
                == "Waiting for postgres +1")
        #expect(StatusAppearance.upPhase(phase: .waiting).symbol == "hourglass")
    }

    @Test func anEnvironmentWithAnActiveOperationReadsBringingUpOrDown() {
        #expect(
            StatusAppearance.environment(FixtureEnvironmentUp.bringingUpSummary).label
                == "Bringing up…")
        #expect(
            StatusAppearance.environment(FixtureEnvironments.summaries[0]).label == "Down")
    }

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "A service the Up works on shows its phase in the Status column; a service whose phase ended shows its instance status"
        ))
    func theStatusColumnShowsThePhaseOfAServiceStillWorkedOn() {
        let rows = ServiceRowModel.rows(
            of: FixtureEnvironmentUp.bringingUpDetail(), instances: [:])
        #expect(
            rows.map(\.appearance.label) == [
                StatusAppearance.instance(
                    lifecycle: .running, readiness: .ready, health: .healthy
                ).label, "Starting…", "Waiting for postgres +1", "Installing…",
            ])
    }

    @Test(
        .evidence(
            covers: ["REQ-021", "NFR-012"],
            expected:
                "The partially ready banner names each service with its reason and offers Set Secret…, the dependency, or Show Operation"
        ))
    func theBannerNamesEachServiceWithItsReasonAndAction() throws {
        var opened: [UUID] = []
        var operations: [UUID] = []
        var needs: [[SecretNeed]] = []
        let items = ServiceRowModel.notReadyItems(
            of: FixtureEnvironmentUp.incompleteDetail(),
            actions: ServiceRowModel.BannerActions(
                open: { opened.append($0) }, openOperation: { operations.append($0) },
                setSecret: { needs.append($0) }))
        #expect(
            items.map(\.reason) == [
                "Start failed: readiness timed out",
                "Needs secret local:erp/search/adminPassword",
            ])
        #expect(items.map { $0.action?.title } == ["Show Operation", "Set Secret…"])
        items[0].action?.perform()
        items[1].action?.perform()
        #expect(operations == [FixtureEnvironmentUp.childID(3)])
        #expect(needs.first?.map(\.reference) == ["local:erp/search/adminPassword"])

        let skipped = ServiceRowModel.notReadyItems(
            of: FixtureEnvironmentUp.notReadyDetail(),
            actions: ServiceRowModel.BannerActions(open: { opened.append($0) }))
        #expect(
            skipped.map(\.reason) == [
                "Start failed: the port is used by another process",
                "Waiting for postgres, which failed", "Waiting for postgres timed out",
                "Waiting for postgres, which is not ready",
            ])
        #expect(skipped[1].action?.title == "Show postgres")
        skipped[1].action?.perform()
        #expect(opened == [FixtureEnvironments.instances[0].id])
        #expect(skipped.allSatisfy { $0.name.isEmpty == false })
    }

    @Test func aReasonWithMoreThanOneReferenceOrAnUnknownCodeStillReads() {
        let item = FixtureEnvironmentUp.searchSecret
        #expect(
            EnvironmentCopy.needsSecretText([item, item])
                == "Needs secret local:erp/search/adminPassword +1")
        #expect(
            EnvironmentCopy.upFailedText(stage: .install, code: "runtime.other", detail: nil)
                == "Install failed: runtime.other")
        #expect(
            EnvironmentCopy.upFailedText(stage: .create, code: nil, detail: "disk full")
                == "Create failed: disk full")
    }

    @Test func theProgressCountsFromTheOperationOrElseTheReport() {
        let running = FixtureEnvironmentUp.upOperation(done: 2, total: 4)
        let fromOperation = EnvironmentDetailPage.progress(operation: running, lastUp: nil)
        #expect(fromOperation.done == 2)
        #expect(fromOperation.total == 4)
        let fromReport = EnvironmentDetailPage.progress(
            operation: nil, lastUp: FixtureEnvironmentUp.bringingUpDetail().lastUp)
        #expect(fromReport.done == 1)
        #expect(fromReport.total == 4)
        #expect(
            EnvironmentCopy.bringingUp(done: 1, total: 4) == "Bringing up… 1 of 4")
        #expect(EnvironmentCopy.bringingUp(done: nil, total: nil) == "Bringing up…")
    }

    @Test func theUpScenariosAnswerTheirOwnDetail() async throws {
        for id in [FixtureScenario.ID.environmentBringingUp, .environmentUpIncomplete] {
            let scenario = FixtureScenario(id)
            guard case .reply(.envGet(let detail)) = scenario.replies["env.get"] else {
                Issue.record("expected env.get in \(id)")
                continue
            }
            #expect(detail.summary.id == FixtureEnvironments.erpID)
            #expect(scenario.operations.contains { $0.kind == .environmentUp })
        }
    }
}
