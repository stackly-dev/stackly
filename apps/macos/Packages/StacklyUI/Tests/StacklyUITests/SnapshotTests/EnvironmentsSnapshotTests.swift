import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Environments screens of spec 0044 (AC-18 to AC-23) in light, dark, and narrow: the
/// list filled, empty, and offline; the detail page partially ready, ready, and down; every
/// manifest banner; the Problems section; the plan sheet with its three verbs, blockers, a
/// name registered elsewhere, and Problems; the Overview section; and the Environment column
/// and inspector row of Instances.
extension SnapshotTests {
    struct EnvironmentsSnapshotTests {
        /// The pages this suite covers, named for the snapshot source rule.
        static let pages = [
            String(describing: EnvironmentsPage.self),
            String(describing: EnvironmentListPage.self),
            String(describing: EnvironmentDetailPage.self),
        ]

        /// The plan sheet, at its own width.
        private static let sheetSize = SnapshotSize.fitting(width: 600, narrowWidth: 560)

        @MainActor
        private static func rig(environments: [EnvironmentSummary] = FixtureEnvironments.summaries)
            async -> PageRig
        {
            let rig = PageRig(
                instances: FixtureEnvironments.instances, environments: environments)
            await rig.load()
            return rig
        }

        // MARK: - List

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Environments list shows each environment with its status and ready count, the empty state with Import…, and the offline state"
            ))
        @MainActor
        func list() async throws {
            let rig = await Self.rig()
            try await assertSnapshot(await rig.environmentsPage(), named: "environments-list")
            let empty = await Self.rig(environments: [])
            try await assertSnapshot(
                await empty.environmentsPage(), named: "environments-empty")
            let offline = PageRig(instances: [])
            try await assertSnapshot(
                await offline.environmentsPage(
                    waiting: FailureContent(
                        title: "Waiting for stacklyd", message: "stacklyd is unavailable."),
                    hasConnection: false
                )
                .environment(
                    \.mutationAvailability,
                    .blocked(reason: "stacklyd is unavailable", since: FixtureClock.now)),
                named: "environments-offline")
        }

        // MARK: - Detail

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The detail page shows the partially ready banner, the Services table, Not in Manifest, and Details; ready and down pages too"
            ))
        @MainActor
        func detail() async throws {
            let rig = await Self.rig()
            try await assertSnapshot(
                await rig.environmentsPage(selected: FixtureEnvironments.erpID),
                named: "environment-partially-ready")
            try await assertSnapshot(
                await rig.environmentsPage(selected: FixtureEnvironments.reportingID),
                named: "environment-ready", variants: [.light])
            try await assertSnapshot(
                await rig.environmentsPage(selected: FixtureEnvironments.billingID),
                named: "environment-down", variants: [.light])
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "Each manifest state shows its banner: changed, can't be read, problems, and a new name"
            ))
        @MainActor
        func manifestBanners() async throws {
            let cases: [(ManifestState, String)] = [
                (.changed, "environment-manifest-changed"),
                (.missing, "environment-manifest-missing"),
                (.invalid, "environment-manifest-invalid"),
                (.nameChanged, "environment-manifest-name-changed"),
            ]
            for (state, name) in cases {
                let rig = await Self.rig()
                rig.script.manifestState = state
                if state == .nameChanged {
                    rig.script.validation = FixtureEnvironments.validation(
                        path: FixtureEnvironments.manifest("erp"), name: "erp-staging")
                }
                try await assertSnapshot(
                    await rig.environmentsPage(selected: FixtureEnvironments.erpID),
                    named: name, variants: [.light])
            }
        }

        @Test(
            .evidence(
                covers: ["VAL-013"],
                expected:
                    "Validate of a manifest that is not valid opens Problems with each finding"
            ))
        @MainActor
        func problems() async throws {
            let rig = await Self.rig()
            rig.script.validation = FixtureEnvironments.validation(
                path: FixtureEnvironments.manifest("erp"), valid: false)
            rig.script.manifestState = .invalid
            try await assertSnapshot(
                await rig.environmentsPage(selected: FixtureEnvironments.erpID, validate: true),
                named: "environment-problems", variants: [.light, .dark])
        }

        // MARK: - Plan sheet

        @MainActor
        private static func sheet(_ rig: PageRig, _ source: EnvironmentPlanPresenter.Source)
            async -> some View
        {
            let presenter = EnvironmentPlanPresenter(
                store: rig.environments, picker: FixtureFilePicker(path: nil))
            await presenter.read(source)
            return EnvironmentPlanSheet(
                presenter: presenter, catalog: rig.catalog, instances: rig.instances,
                home: FixtureDetail.home, reviewProvider: { _ in })
        }

        @Test(
            .evidence(
                covers: ["REQ-020", "AC-020"],
                expected:
                    "The import plan of the ERP manifest shows six creates with their installs and secrets, then Cancel and Import"
            ))
        @MainActor
        func freshPlan() async throws {
            let rig = await Self.rig()
            try await assertSnapshot(
                await Self.sheet(rig, .file(FixtureEnvironments.manifest("erp"))),
                named: "environment-plan-import", size: Self.sheetSize)
        }

        @Test(
            .evidence(
                covers: ["AC-020", "VAL-013"],
                expected:
                    "The plan of a registered manifest shows Keep, Start, and Create, a difference, approval, and the blockers, with Done only"
            ))
        @MainActor
        func registeredPlan() async throws {
            let rig = await Self.rig()
            try await assertSnapshot(
                await Self.sheet(
                    rig, .review(environment: FixtureEnvironments.erpID, name: "erp-development")),
                named: "environment-plan-blocked", size: Self.sheetSize)
        }

        @Test @MainActor func invalidAndTakenPlans() async throws {
            let invalid = await Self.rig()
            invalid.script.plan = FixtureEnvironments.invalidPlan()
            try await assertSnapshot(
                await Self.sheet(invalid, .file(FixtureEnvironments.manifest("broken"))),
                named: "environment-plan-invalid", size: Self.sheetSize, variants: [.light])
            let taken = await Self.rig()
            taken.script.plan = FixtureEnvironments.nameTakenPlan()
            try await assertSnapshot(
                await Self.sheet(taken, .file(FixtureEnvironments.manifest("erp-copy"))),
                named: "environment-plan-name-taken", size: Self.sheetSize, variants: [.light])
        }

        // MARK: - Other surfaces

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Overview shows an Environments section above Operations; Instances shows the Environment column"
            ))
        @MainActor
        func otherSurfaces() async throws {
            let rig = await Self.rig()
            try await assertSnapshot(rig.overviewPage(), named: "overview-environments")
            try await assertSnapshot(
                rig.instancesPage(
                    selection: FixtureEnvironments.instances[0].id.uuidString.lowercased()),
                named: "instances-environments")
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false), named: "instances-environment-column",
                variants: [.light])
        }
    }
}
