import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The screens of Down (spec 0047, AC-20, AC-21) in light, dark, and narrow: the detail page
/// while a Down runs with "Bringing down… 1 of 2", Cancel, and Show in Activity; the list with
/// "Bringing down…"; and the Operations page with the Down and its Partial label.
extension SnapshotTests {
    struct EnvironmentDownSnapshotTests {
        /// The pages this suite covers, named for the snapshot source rule.
        static let pages = [
            String(describing: EnvironmentDetailPage.self),
            String(describing: EnvironmentListPage.self),
        ]

        @MainActor
        private static func rig(
            environments: [EnvironmentSummary],
            operations: [OperationSummary] = FixtureScenario.operationCases,
            detail: EnvironmentDetail? = nil
        ) async -> PageRig {
            let rig = PageRig(
                instances: FixtureEnvironments.instances, operations: operations,
                environments: environments)
            if let detail { rig.script.environmentDetails[detail.summary.id] = detail }
            await rig.load()
            return rig
        }

        @Test(
            .evidence(
                covers: ["REQ-022", "NFR-012"],
                expected:
                    "While a Down runs the detail page shows Bringing down… 1 of 2 with Cancel and Show in Activity"
            ))
        @MainActor
        func detailWhileBringingDown() async throws {
            let rig = await Self.rig(
                environments: FixtureEnvironmentDown.bringingDownSummaries,
                operations: [FixtureEnvironmentDown.downOperation()]
                    + FixtureScenario.operationCases,
                detail: FixtureEnvironmentDown.bringingDownDetail())
            try await assertSnapshot(
                await rig.environmentsPage(selected: FixtureEnvironments.erpID),
                named: "environment-bringing-down")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Environments list shows Bringing down… for the environment whose Down runs"
            ))
        @MainActor
        func listWhileBringingDown() async throws {
            let rig = await Self.rig(environments: FixtureEnvironmentDown.bringingDownSummaries)
            try await assertSnapshot(
                await rig.environmentsPage(), named: "environments-list-bringing-down",
                variants: [.light, .dark])
        }

        @Test(
            .evidence(
                covers: ["REQ-022", "NFR-012"],
                expected:
                    "The result of a Down that kept a shared instance and one you started names each with its reason: used by reporting, started by you"
            ))
        @MainActor
        func resultThatKeptSharedAndManualInstances() async throws {
            let down = FixtureEnvironmentDown.self
            let preview = down.preview()
            let made = EnvironmentDownModel.feedback(
                down.downOperation(state: .succeeded, result: down.result(stopped: 1)),
                name: "erp-development", kept: preview.keep.count,
                keptLine: EnvironmentCopy.keepLine(preview.keep), bringDownAgain: {},
                showOperation: {})
            let feedback = try #require(made)
            try await assertSnapshot(
                FeedbackCard(feedback: feedback, center: FeedbackCenter())
                    .padding(Spacing.s12),
                named: "environment-down-kept-feedback",
                size: SnapshotSize.fitting(width: 520, narrowWidth: 400))
        }

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected:
                    "Operations shows Bring down environment erp-development with Partial and 1 of 2 instances stopped, and its two child stops nested under it"
            ))
        @MainActor
        func activityWithAPartialDown() async throws {
            let rig = PageRig(environments: FixtureEnvironments.summaries)
            await rig.loadInstances()
            await rig.environments.store.resnapshot()
            let fixture = FixtureOperations.Case.environmentDown
            let center = FixtureOperations.center(fixture)
            await center.store.refresh()
            let model = OperationsPageModel(
                service: FixtureOperations.service(fixture), center: center)
            await model.history.reload()
            let page = OperationsPage(
                model: model, operations: center, instances: rig.instances,
                environments: rig.environments, actions: rig.actions, waiting: nil,
                hasConnection: true, reconnect: {}, openDiagnostics: {}
            )
            .actionHost()
            .frame(height: 900)
            try await assertSnapshot(
                page, named: fixture.rawValue,
                size: SnapshotSize.fitting(width: 1_044, narrowWidth: 664))
        }
    }
}
