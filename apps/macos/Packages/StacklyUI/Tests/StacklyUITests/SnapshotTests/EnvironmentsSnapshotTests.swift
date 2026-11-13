import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Environments screens of spec 0044 (AC-18 to AC-23) in light, dark, and narrow: the
/// list filled, empty, and offline; the detail page partially ready, ready, and down; every
/// manifest banner; the Problems section; the plan sheet with its three verbs, blockers, a
/// name registered elsewhere, and Problems; the Overview section with Show All; and the
/// Environment column and inspector row of Instances. From spec 0045 (AC-18, AC-19): the Lock
/// and Local override rows in every state, a blocked lock, and the plan sheet with a lock and
/// overrides; and the detail page while stacklyd is unavailable.
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
                presenter: presenter, catalog: rig.catalog, secrets: rig.detail.secrets,
                instances: rig.instances, home: FixtureDetail.home, reviewProvider: { _ in })
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
                    "The plan of a registered manifest shows Keep, Start, and Create, a difference, approval, the blockers, Set… on a missing secret, and Cancel with Bring Up off for its blockers"
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

        // MARK: - Lock and local override (spec 0045)

        @Test(
            .evidence(
                covers: ["NFR-012", "REQ-023"],
                expected:
                    "Details shows the Lock row in sync, out of date, not locked, and unreadable, and the Local override row with overrides, problems, or None"
            ))
        @MainActor
        func lockRows() async throws {
            let cases: [(EnvLockInfo, EnvLocalInfo, String, [SnapshotVariant])] = [
                (
                    FixtureEnvironments.lockInfo(.stale),
                    FixtureEnvironments.localInfo(.invalid, problems: 2),
                    "environment-lock-stale", [.light, .dark, .narrow]
                ),
                (
                    FixtureEnvironments.lockInfo(.absent), FixtureEnvironments.localInfo(nil),
                    "environment-lock-absent", [.light]
                ),
                (
                    FixtureEnvironments.lockInfo(.unreadable),
                    FixtureEnvironments.localInfo(.unreadable), "environment-lock-unreadable",
                    [.light]
                ),
            ]
            for (lock, local, name, variants) in cases {
                let rig = await Self.rig()
                rig.script.environmentDetails[FixtureEnvironments.erpID] =
                    FixtureEnvironments.detail(FixtureEnvironments.erpID, files: (lock, local))
                try await assertSnapshot(
                    await rig.environmentsPage(selected: FixtureEnvironments.erpID),
                    named: name, variants: variants)
            }
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "A lock that a provider blocks lists the blocker with its hint in a critical banner"
            ))
        @MainActor
        func lockBlocked() async throws {
            let rig = await Self.rig()
            rig.script.lockResult = FixtureEnvironments.lockBlocked()
            try await assertSnapshot(
                await rig.environmentsPage(
                    selected: FixtureEnvironments.erpID, lock: LockRefresh.none),
                named: "environment-lock-blocked", variants: [.light, .dark])
        }

        @Test(
            .evidence(
                covers: ["REQ-023", "NFR-012"],
                expected:
                    "The plan sheet marks creates from the lock, shows the local overrides and the lock difference, the stale lock caution, and the local file warning"
            ))
        @MainActor
        func lockedPlan() async throws {
            let rig = await Self.rig()
            rig.script.plan = FixtureEnvironments.lockedPlan()
            try await assertSnapshot(
                await Self.sheet(
                    rig, .review(environment: FixtureEnvironments.erpID, name: "erp-development")),
                named: "environment-plan-locked", size: Self.sheetSize)
            let blocked = await Self.rig()
            blocked.script.plan = FixtureEnvironments.lockedBlockedPlan()
            try await assertSnapshot(
                await Self.sheet(
                    blocked,
                    .review(environment: FixtureEnvironments.erpID, name: "erp-development")),
                named: "environment-plan-locked-blocked", size: Self.sheetSize,
                variants: [.light, .dark])
            // A manifest that is not valid shows only Problems: no " · locked", stale lock, or
            // local override, though the lock reads in_sync (spec 0045 K23).
            let invalid = await Self.rig()
            invalid.script.plan = FixtureEnvironments.invalidLockedPlan()
            try await assertSnapshot(
                await Self.sheet(
                    invalid,
                    .review(environment: FixtureEnvironments.erpID, name: "erp-development")),
                named: "environment-plan-invalid-locked", size: Self.sheetSize,
                variants: [.light])
        }

        // MARK: - Offline and Show All (spec 0044)

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "An environment opened while stacklyd is unavailable shows Waiting for stacklyd"
            ))
        @MainActor
        func detailOffline() async throws {
            let rig = await Self.rig()
            rig.script.environmentDetailFailure = .notSent(ClientErrorCatalog.notConnected())
            try await assertSnapshot(
                await rig.environmentsPage(
                    selected: FixtureEnvironments.erpID,
                    waiting: FailureContent(
                        title: "Waiting for stacklyd", message: "stacklyd is unavailable."),
                    hasConnection: false
                )
                .environment(
                    \.mutationAvailability,
                    .blocked(reason: "stacklyd is unavailable", since: FixtureClock.now)),
                named: "environment-offline")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Overview lists five environments by name and Show All when there are more"
            ))
        @MainActor
        func overviewShowAll() async throws {
            let rig = await Self.rig(environments: FixtureEnvironments.manySummaries)
            try await assertSnapshot(
                rig.overviewPage(), named: "overview-environments-show-all",
                variants: [.light, .dark])
        }

        // MARK: - Sidebar

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The sidebar shows one child row per environment under Environments with its status symbol, name, and ready count"
            ))
        @MainActor
        func sidebarRows() async throws {
            let runtime = try await settledRuntime(FixtureScenario(.environmentPartiallyReady))
            defer { runtime.willTerminate() }
            let model = runtime.model
            try await waitUntil { model.environments.environments?.count == 3 }
            let shell = AppShell(
                presentation: model.presentation, diagnostics: model.diagnostics,
                counts: model.sidebarCounts, instances: model.instances,
                catalog: model.catalog, runtimes: model.runtimes,
                environments: model.environments, operations: model.operations,
                capacity: model.capacity, instanceActions: model.instanceActions,
                completedRounds: model.sync.completedRounds, reconnect: {},
                refreshDiagnostics: {}
            )
            .environment(model)
            .environment(\.fixtureNow, FixtureClock.now)
            .frame(width: 1280, height: 800)
            try await assertSnapshot(
                shell, named: "shell-environments-sidebar",
                size: .fitting(width: 1280, narrowWidth: 900), variants: [.light, .dark])
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
