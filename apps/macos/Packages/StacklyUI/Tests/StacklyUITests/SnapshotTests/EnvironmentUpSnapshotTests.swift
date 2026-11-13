import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The screens of Up (spec 0046, AC-19 to AC-22) in light, dark, and narrow: the plan sheet
/// with Bring Up on, after `env.plan_changed`, and while stacklyd is unavailable with its
/// controls off and the reason; the detail page while an Up runs, after an Up that left
/// services not ready with each reason, and after one that left none ready or was
/// interrupted; the list with "Bringing up…"; and the Operations page with the Up, its
/// nested children, and the Partial label.
extension SnapshotTests {
    struct EnvironmentUpSnapshotTests {
        /// The pages this suite covers, named for the snapshot source rule.
        static let pages = [
            String(describing: EnvironmentDetailPage.self),
            String(describing: EnvironmentListPage.self),
            String(describing: OperationsPage.self),
        ]

        /// The plan sheet, at its own width.
        private static let sheetSize = SnapshotSize.fitting(width: 600, narrowWidth: 560)

        @MainActor
        private static func rig(
            environments: [EnvironmentSummary] = FixtureEnvironments.summaries,
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

        // MARK: - Plan sheet

        @MainActor
        private static func sheet(_ rig: PageRig, notice: Bool = false) async -> some View {
            let presenter = EnvironmentPlanPresenter(
                store: rig.environments, picker: FixtureFilePicker(path: nil))
            let source = EnvironmentPlanPresenter.Source.review(
                environment: FixtureEnvironments.erpID, name: "erp-development")
            await presenter.read(source, notice: notice ? EnvironmentCopy.planChanged : nil)
            return EnvironmentPlanSheet(
                presenter: presenter, catalog: rig.catalog, secrets: rig.detail.secrets,
                instances: rig.instances, home: FixtureDetail.home, reviewProvider: { _ in })
        }

        @Test(
            .evidence(
                covers: ["REQ-021", "NFR-012"],
                expected:
                    "The plan sheet of a registered manifest has Cancel and Bring Up on, a Needs secret line with Set…, and the footnote about dependency order and partially ready"
            ))
        @MainActor
        func planWithBringUp() async throws {
            let rig = await Self.rig()
            rig.script.plan = FixtureEnvironmentUp.upPlan()
            try await assertSnapshot(
                await Self.sheet(rig), named: "environment-plan-up", size: Self.sheetSize)
            try await assertSnapshot(
                await Self.sheet(rig, notice: true), named: "environment-plan-changed",
                size: Self.sheetSize, variants: [.light])
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "While stacklyd is unavailable the loaded plan sheet keeps the plan, Bring Up and Set… are off, and the footer says why"
            ))
        @MainActor
        func planWhileOffline() async throws {
            let rig = await Self.rig()
            rig.script.plan = FixtureEnvironmentUp.upPlan()
            try await assertSnapshot(
                await Self.sheet(rig)
                    .environment(
                        \.mutationAvailability,
                        .blocked(reason: "stacklyd is unavailable", since: FixtureClock.now)),
                named: "environment-plan-up-offline", size: Self.sheetSize,
                variants: [.light, .dark])
        }

        // MARK: - Detail page

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "While an Up runs the detail page shows Bringing up… 1 of 4 with Cancel and Show in Activity, and each service still worked on shows its phase"
            ))
        @MainActor
        func detailWhileBringingUp() async throws {
            let rig = await Self.rig(
                environments: FixtureEnvironmentUp.bringingUpSummaries,
                operations: [FixtureEnvironmentUp.upOperation()]
                    + FixtureScenario.operationCases,
                detail: FixtureEnvironmentUp.bringingUpDetail())
            try await assertSnapshot(
                await rig.environmentsPage(selected: FixtureEnvironments.erpID),
                named: "environment-bringing-up")
        }

        @Test(
            .evidence(
                covers: ["REQ-021", "NFR-012"],
                expected:
                    "After an Up the partially ready banner names each service that is not ready with its reason, Start failed with Show Operation and Needs secret with Set Secret…"
            ))
        @MainActor
        func detailAfterAnIncompleteUp() async throws {
            let rig = await Self.rig(
                environments: FixtureEnvironmentUp.incompleteSummaries,
                detail: FixtureEnvironmentUp.incompleteDetail())
            try await assertSnapshot(
                await rig.environmentsPage(selected: FixtureEnvironments.erpID),
                named: "environment-up-incomplete")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "An Up that left no service ready shows Not ready with each dependency reason; an interrupted one adds the line to bring it up again"
            ))
        @MainActor
        func detailWithNothingReady() async throws {
            for (state, name) in [
                (UpState.failed, "environment-up-failed"),
                (.interrupted, "environment-up-interrupted"),
            ] {
                let detail = FixtureEnvironmentUp.notReadyDetail(state)
                let rig = await Self.rig(
                    environments: FixtureEnvironments.summaries.map {
                        $0.id == FixtureEnvironments.erpID ? detail.summary : $0
                    }, detail: detail)
                try await assertSnapshot(
                    await rig.environmentsPage(selected: FixtureEnvironments.erpID),
                    named: name, variants: [.light])
            }
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Environments list shows Bringing up… for the environment whose Up runs, with its ready count"
            ))
        @MainActor
        func listWhileBringingUp() async throws {
            let rig = await Self.rig(environments: FixtureEnvironmentUp.bringingUpSummaries)
            try await assertSnapshot(
                await rig.environmentsPage(), named: "environments-list-bringing-up",
                variants: [.light, .dark])
        }

        // MARK: - Activity

        @Test(
            .evidence(
                covers: ["REQ-054", "NFR-012"],
                expected:
                    "Operations shows Bring up environment erp-development with Partial and 2 of 4 services ready, and its three child starts nested under it"
            ))
        @MainActor
        func activityWithNestedChildren() async throws {
            let rig = PageRig(environments: FixtureEnvironments.summaries)
            await rig.loadInstances()
            await rig.environments.store.resnapshot()
            let fixture = FixtureOperations.Case.environmentUp
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

extension SnapshotTests {
    /// The toolbar controls of the environment detail page (spec 0046, AC-19; spec 0047,
    /// AC-20): the off screen snapshot window draws no toolbar, so the same actions the
    /// toolbar builds are drawn with the same buttons in a row: Validate, Down, and Up…, on
    /// and off. As in the real toolbar, a reason shows only as the button's tooltip.
    struct EnvironmentUpToolbarSnapshotTests {
        @MainActor
        private static func row(_ summary: EnvironmentSummary) -> some View {
            let validate = EnvironmentDetailPage.validateAction(
                location: summary.manifestLocation, isValidating: false, perform: {})
            let down = EnvironmentDetailPage.downAction(
                summary: summary, isBusy: false, perform: {})
            let up = EnvironmentDetailPage.upAction(
                summary: summary, manifestState: .inSync, problemCount: 0, declaredName: nil,
                perform: {})
            return HStack(spacing: Spacing.s8) {
                ActionButton(validate, prominence: .secondary, showsReason: false)
                ActionButton(down, prominence: .secondary, showsReason: false)
                ActionButton(up, prominence: .primary, showsReason: false)
            }
            .padding(Spacing.s12)
            .actionHost()
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail toolbar has Validate, Down, and the accent Up…; Up… is off while an Up runs, and Down and Up… are off while a Down runs, each with its reason only as a tooltip"
            ))
        @MainActor
        func toolbarWithUp() async throws {
            let size = SnapshotSize.fitting(width: 420, narrowWidth: 420)
            try await assertSnapshot(
                Self.row(FixtureEnvironments.summaries[1]), named: "environment-toolbar-up",
                size: size, variants: [.light, .dark])
            try await assertSnapshot(
                Self.row(FixtureEnvironmentUp.bringingUpSummary),
                named: "environment-toolbar-up-off", size: size, variants: [.light, .dark])
            try await assertSnapshot(
                Self.row(FixtureEnvironmentDown.bringingDownSummary),
                named: "environment-toolbar-down-off", size: size, variants: [.light, .dark])
        }
    }
}
