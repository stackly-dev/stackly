import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The edges of the environment screens: Locate… of a file that declares another name, Import
/// as New of the `name_changed` banner, and the Environment column of Instances (spec 0044,
/// AC-21, AC-23).
@MainActor
struct EnvironmentEdgeTests {
    let rig = PageRig(environments: FixtureEnvironments.summaries)

    @Test func locateOfAFileThatDeclaresAnotherNameSaysRelinkMismatch() async throws {
        rig.script.environmentOutcome = .rejected(
            ErrorBody(
                code: "env.relink_mismatch", category: .conflict,
                message: "the file declares another environment", retryable: false,
                resource: nil, fields: nil, hint: nil,
                details: [
                    ErrorDetail(key: "declared", value: "erp-staging"),
                    ErrorDetail(key: "registered", value: "erp-development"),
                ], operationId: nil, rule: nil, items: nil))
        let model = EnvironmentDetailModel(
            id: FixtureEnvironments.erpID, store: rig.environments, newKey: { "key-1" })
        await model.load(checkManifest: true)

        let feedback = await model.locate("/Users/stackly-fixture/Other/stackly.yaml")

        #expect(feedback?.code == "env.relink_mismatch")
        #expect(feedback?.tone == .critical)
        #expect(model.detail?.summary.name == "erp-development")
        #expect(rig.script.environmentSent.count == 1)
        guard case .envImport(let params) = try #require(rig.script.environmentSent.first)
        else {
            Issue.record("expected env.import")
            return
        }
        #expect(params.relinkEnvironmentId == FixtureEnvironments.erpID)
    }

    @Test func importAsNewReadsThePlanOfTheSameFileAndSendsNothing() async throws {
        let path = FixtureEnvironments.manifest("erp")
        let presenter = EnvironmentPlanPresenter(
            store: rig.environments, picker: FixtureFilePicker(path: nil), newKey: { "key-1" })

        presenter.importFile(path)
        try await waitUntil { presenter.plan != nil }

        #expect(presenter.isImport)
        #expect(presenter.source == .file(path))
        #expect(rig.script.environmentSent.isEmpty)
    }

    private var rows: [InstanceRowModel] {
        InstanceRowModel.rows(of: FixtureEnvironments.instances, catalog: nil)
    }

    private func sorted(_ order: SortOrder) -> [InstanceRowModel] {
        rows.sorted(using: InstanceSortComparator(.environment, order: order))
    }

    @Test func theEnvironmentColumnSortsByTheFirstEnvironmentWithNoneLast() {
        let forward = sorted(.forward)
        let reverse = sorted(.reverse)
        let members = rows.filter { $0.environmentName != nil }.count
        #expect(members == 4)

        #expect(
            forward.prefix(members).map(\.name) == ["mysql-erp", "cache", "pg17-dev", "queue"])
        #expect(
            reverse.prefix(members).map(\.name) == ["cache", "pg17-dev", "queue", "mysql-erp"],
            "a tie stays by name in both directions")
        #expect(forward.dropFirst(members).allSatisfy { $0.environmentName == nil })
        #expect(
            reverse.dropFirst(members).allSatisfy { $0.environmentName == nil },
            "none is last in both directions")
        #expect(forward.first { $0.name == "pg17-dev" }?.environmentText.hasPrefix("erp") == true)
    }
}
