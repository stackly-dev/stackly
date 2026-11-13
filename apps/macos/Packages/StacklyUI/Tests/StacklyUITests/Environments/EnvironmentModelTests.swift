import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The detail page model: manifest state, Validate, Reload, Locate…, and Remove (spec 0044,
/// AC-21, AC-22).
@MainActor
struct EnvironmentDetailModelTests {
    let rig = PageRig(environments: FixtureEnvironments.summaries)

    private func model(_ id: UUID = FixtureEnvironments.erpID) -> EnvironmentDetailModel {
        EnvironmentDetailModel(id: id, store: rig.environments, newKey: { "key-1" })
    }

    @Test func aReadWithoutTheFileKeepsTheManifestStateOfTheLastCheck() async {
        rig.script.manifestState = .changed
        let model = model()
        await model.load(checkManifest: true)
        #expect(model.manifestState == .changed)
        await model.load(checkManifest: false)
        #expect(model.manifestState == .changed)
        #expect(model.detail?.summary.name == "erp-development")
    }

    @Test func aNameChangedManifestReadsTheNameTheFileDeclares() async {
        rig.script.manifestState = .nameChanged
        rig.script.validation = FixtureEnvironments.validation(
            path: FixtureEnvironments.manifest("erp"), name: "erp-staging")
        let model = model()
        await model.load(checkManifest: true)
        #expect(model.declaredName == "erp-staging")
    }

    @Test(
        .evidence(
            covers: ["REQ-020", "VAL-013"],
            expected: "Validate shows success feedback for a valid manifest and Problems otherwise"
        ))
    func validateShowsFeedbackOrProblems() async {
        let model = model()
        await model.load(checkManifest: true)
        let feedback = await model.validate()
        #expect(
            feedback?.message
                == "erp-development: schema stackly/v1alpha1 valid · no cycles · secrets referenced."
        )
        #expect(model.findings == nil)

        rig.script.validation = FixtureEnvironments.validation(
            path: FixtureEnvironments.manifest("erp"), valid: false)
        #expect(await model.validate() == nil)
        #expect(model.findings?.map(\.code) == ["dependency_cycle", "secret_literal"])
        #expect(rig.script.environmentSent.isEmpty)
    }

    @Test func reloadImportsTheFileWithTheDigestValidateRead() async throws {
        let model = model()
        await model.load(checkManifest: true)
        let feedback = await model.reloadManifest()
        #expect(feedback?.tone == .positive)
        let sent = try #require(rig.script.environmentSent.first)
        guard case .envImport(let params) = sent else {
            Issue.record("expected env.import, got \(sent)")
            return
        }
        #expect(params.path == FixtureEnvironments.manifest("erp"))
        #expect(params.expectedManifestSha256 == String(repeating: "d", count: 64))
        #expect(params.relinkEnvironmentId == nil)
    }

    @Test func reloadOfAnInvalidFileOpensProblemsAndSendsNothing() async {
        rig.script.validation = FixtureEnvironments.validation(
            path: FixtureEnvironments.manifest("erp"), valid: false)
        let model = model()
        await model.load(checkManifest: true)
        #expect(await model.reloadManifest() == nil)
        #expect(model.findings?.isEmpty == false)
        #expect(rig.script.environmentSent.isEmpty)
    }

    @Test func locateImportsTheChosenFileWithThisEnvironment() async throws {
        let model = model()
        await model.load(checkManifest: true)
        _ = await model.locate("/Users/stackly-fixture/Moved/stackly.yaml")
        guard case .envImport(let params) = try #require(rig.script.environmentSent.first)
        else {
            Issue.record("expected env.import")
            return
        }
        #expect(params.path == "/Users/stackly-fixture/Moved/stackly.yaml")
        #expect(params.relinkEnvironmentId == FixtureEnvironments.erpID)
    }

    @Test(
        .evidence(
            covers: ["REQ-020"],
            expected:
                "Remove sends env.remove with the revision shown; an active lease says Bring it down first."
        ))
    func removeSendsTheRevisionAndExplainsAnActiveLease() async throws {
        let model = model()
        await model.load(checkManifest: true)
        guard case .removed(let feedback) = await model.remove() else {
            Issue.record("expected the environment to be removed")
            return
        }
        #expect(feedback.message == "Removed “erp-development”.")
        guard case .envRemove(let params) = try #require(rig.script.environmentSent.first) else {
            Issue.record("expected env.remove")
            return
        }
        #expect(params.expectedRevision == 2)

        rig.script.environmentOutcome = .rejected(
            ErrorBody(
                code: "env.lease_active", category: .conflict,
                message: "the environment still holds 1 lease", retryable: false, resource: nil,
                fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
                items: [
                    ErrorItem(
                        kind: "instance",
                        fields: [ErrorDetail(key: "name", value: "pg17-dev")])
                ]))
        guard case .kept(let kept) = await model.remove() else {
            Issue.record("expected the environment to stay")
            return
        }
        #expect(kept?.message == "Bring it down first.")
        #expect(kept?.detail == "pg17-dev")
    }
}

/// Import… and the plan sheet (spec 0044, AC-20, AC-21, AC-25).
@MainActor
struct EnvironmentPlanPresenterTests {
    let rig = PageRig()
    let path = FixtureEnvironments.manifest("erp")

    private func presenter(path: String?) -> EnvironmentPlanPresenter {
        EnvironmentPlanPresenter(
            store: rig.environments, picker: FixtureFilePicker(path: path),
            newKey: { "key-1" })
    }

    private func waitForPlan(_ presenter: EnvironmentPlanPresenter) async throws {
        try await waitUntil { presenter.plan != nil || presenter.phase == .idle }
    }

    @Test(
        .evidence(
            covers: ["REQ-020", "AC-020", "AC-080"],
            expected:
                "Import… reads env.plan for the chosen file and sends nothing until Import, then env.import with the plan's manifestSha256"
        ))
    func importReadsThePlanThenSendsTheDigestItShowed() async throws {
        let presenter = presenter(path: path)
        var imported: EnvironmentDetail?
        presenter.imported = { imported = $0 }
        presenter.begin()
        try await waitForPlan(presenter)
        #expect(presenter.isImport)
        #expect(presenter.name == "erp-development")
        #expect(rig.script.environmentSent.isEmpty)
        let plan = try #require(presenter.plan)
        #expect(plan.items?.filter { $0.action == .create }.count == 6)
        #expect(
            plan.items?.filter { $0.runtime?.install == true }.count == 6)

        await presenter.importPlan()
        guard case .envImport(let params) = try #require(rig.script.environmentSent.first) else {
            Issue.record("expected env.import")
            return
        }
        #expect(params.path == path)
        #expect(params.expectedManifestSha256 == plan.validation.manifestSha256)
        #expect(params.relinkEnvironmentId == nil)
        #expect(presenter.phase == .idle)
        #expect(imported != nil)
    }

    @Test func cancelInTheFilePanelLeavesNoSheet() async throws {
        let presenter = presenter(path: nil)
        presenter.begin()
        try await waitUntil { presenter.phase == .idle }
        #expect(presenter.isPresented == false)
    }

    @Test func anInvalidManifestCantBeImported() async throws {
        rig.script.plan = FixtureEnvironments.invalidPlan(path: path)
        let presenter = presenter(path: path)
        await presenter.read(.file(path))
        let plan = try #require(presenter.plan)
        #expect(EnvironmentPlanPresenter.importBlockedReason(plan) != nil)
        await presenter.importPlan()
        #expect(rig.script.environmentSent.isEmpty)
    }

    @Test func aBlockerThatIsNotTheNameDoesNotStopTheImport() {
        #expect(
            EnvironmentPlanPresenter.importBlockedReason(FixtureEnvironments.registeredPlan())
                == nil)
    }

    @Test func useThisFileRelinksTheEnvironmentOfTheTakenName() async throws {
        rig.script.plan = FixtureEnvironments.nameTakenPlan(path: path)
        let presenter = presenter(path: path)
        await presenter.read(.file(path))
        let plan = try #require(presenter.plan)
        #expect(EnvironmentPlanPresenter.importBlockedReason(plan) != nil)
        await presenter.importPlan()
        #expect(rig.script.environmentSent.isEmpty)
        await presenter.importPlan(relink: true)
        guard case .envImport(let params) = try #require(rig.script.environmentSent.first) else {
            Issue.record("expected env.import")
            return
        }
        #expect(params.relinkEnvironmentId == FixtureEnvironments.erpID)
    }

    @Test func aRefusedImportKeepsThePlanAndSaysWhy() async throws {
        let refusal = ErrorBody(
            code: "env.manifest_changed", category: .conflict,
            message: "the manifest changed since the plan", retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
        rig.script.environmentOutcome = .rejected(refusal)
        let presenter = presenter(path: path)
        await presenter.read(.file(path))
        await presenter.importPlan()
        guard case .importFailed(_, _, let error) = presenter.phase else {
            Issue.record("expected the sheet to stay with the refusal")
            return
        }
        #expect(error.code == "env.manifest_changed")
    }

    @Test func reviewReadsTheRegisteredManifestAndHasNoImport() async throws {
        let presenter = presenter(path: nil)
        await presenter.read(
            .review(environment: FixtureEnvironments.erpID, name: "erp-development"))
        #expect(presenter.isImport == false)
        #expect(presenter.plan?.registration == .registered)
        await presenter.importPlan()
        #expect(rig.script.environmentSent.isEmpty)
    }
}

/// The words of the plan sheet and the Services table (spec 0044, *Layar*).
@MainActor
struct EnvironmentCopyTests {
    private let names: (String) -> String = { provider in
        provider == "official/postgresql" ? "PostgreSQL" : provider
    }

    @Test func aCreateRowNamesTheRuntimeAndTheEndpoint() throws {
        let item = try #require(FixtureEnvironments.freshPlan().items?.first)
        #expect(
            PlanItemRow.line(item, displayName: names)
                == "postgres → erp-development-postgres · PostgreSQL 18.6 · 127.0.0.1:5432")
        #expect(
            PlanItemRow.notes(item, displayName: names)
                == ["Installs PostgreSQL 18.6", "Uses secret local:erp/postgres/superuserPassword"])
        #expect(PlanItemRow.badge(.create).label == "+ Create")
    }

    @Test func anAutoPortAndAMissingSecretReadAsTheSpecSays() throws {
        let items = try #require(FixtureEnvironments.freshPlan().items)
        let traefik = try #require(items.last)
        #expect(PlanItemRow.line(traefik, displayName: names).hasSuffix("127.0.0.1:auto port"))
        let mysql = items[1]
        #expect(
            PlanItemRow.notes(mysql, displayName: names).last
                == "Needs secret: local:erp/mysql/rootPassword")
        let valkey = items[4]
        #expect(
            PlanItemRow.notes(valkey, displayName: names).contains { $0.contains("secret") }
                == false)
    }

    @Test func keepAndStartRowsAndTheirDifferences() throws {
        let items = try #require(FixtureEnvironments.registeredPlan().items)
        #expect(
            PlanItemRow.line(items[0], displayName: names, version: "17.6")
                == "postgres → pg17-dev · 17.6")
        #expect(
            PlanItemRow.notes(items[0], displayName: names) == ["Differs from manifest: runtime"])
        #expect(
            PlanItemRow.line(items[1], displayName: names) == "queue → queue · wait for readiness")
        #expect(PlanItemRow.badge(.start).label == "▸ Start")
        #expect(PlanItemRow.badge(.keep).label == "= Keep")
    }

    @Test func eachBlockerIsItsOwnBannerItem() {
        let items = EnvironmentPlanContent.items(
            FixtureEnvironments.registeredPlan().blockers ?? [])
        #expect(items.map(\.name) == ["search", "search (provider.approval_required)"])
        #expect(
            items.first?.reason
                == "Port 5432 is already used by instance pg17-dev. Choose another port in the manifest."
        )
    }

    @Test func theSheetFollowsTheStartOrder() {
        var plan = FixtureEnvironments.registeredPlan()
        plan = EnvPlan(
            validation: plan.validation, registration: plan.registration,
            environment: plan.environment, registeredLocation: plan.registeredLocation,
            applicable: plan.applicable, planDigest: plan.planDigest, items: plan.items,
            order: ["search", "postgres", "queue"], blockers: plan.blockers,
            detached: plan.detached, needsSecrets: plan.needsSecrets, lock: nil, local: nil)
        #expect(
            EnvironmentPlanContent.ordered(plan).map(\.alias) == ["search", "postgres", "queue"])
    }

    @Test func serviceRowsReadTheirColumns() throws {
        let detail = try #require(FixtureEnvironments.detail(FixtureEnvironments.erpID))
        let rows = ServiceRowModel.rows(of: detail, instances: [:])
        #expect(
            rows.map(\.selector).prefix(3) == [
                "official/postgresql@17", "official/valkey@stable", "instanceRef: queue",
            ])
        #expect(rows.last?.instanceText == "Will be created")
        #expect(rows.last?.appearance == .serviceNotCreated)
        #expect(rows.map(\.ownership) == ["Dedicated", "Dedicated", "Shared ref", "Dedicated"])
        let items = ServiceRowModel.notReadyItems(of: detail, open: { _ in })
        #expect(items.map(\.name) == ["cache (cache)", "queue (queue)", "search"])
        #expect(items.map(\.reason) == ["stopping", "failed: crash loop", "not created"])
    }

    @Test func theColumnsNarrowVersionFirstThenSelector() {
        #expect(ServiceColumnSet(width: 900) == .full)
        #expect(ServiceColumnSet(width: 700).showsVersion == false)
        #expect(ServiceColumnSet(width: 700).showsSelector)
        #expect(ServiceColumnSet(width: 500).showsSelector == false)
    }

    @Test func theListSaysWhenThereIsNothingAndWhenItCantLoad() {
        let empty = EnvironmentListPage.state(
            list: [], error: nil, unsupported: false, waiting: nil, loadedAt: FixtureClock.now,
            isRefreshing: false, importAction: nil)
        guard case .empty(let content, _) = empty else {
            Issue.record("expected the empty state")
            return
        }
        #expect(content.title == "No Environments")
        let waiting = FailureContent(title: "Waiting for stacklyd", message: "x")
        let offline = EnvironmentListPage.state(
            list: nil, error: nil, unsupported: false, waiting: waiting, loadedAt: nil,
            isRefreshing: false, importAction: nil)
        guard case .failed(let failure) = offline else {
            Issue.record("expected the failed state")
            return
        }
        #expect(failure == waiting)
        let unsupported = EnvironmentListPage.state(
            list: nil, error: nil, unsupported: true, waiting: nil, loadedAt: nil,
            isRefreshing: false, importAction: nil)
        guard case .unsupported = unsupported else {
            Issue.record("expected the unsupported state")
            return
        }
    }
}
