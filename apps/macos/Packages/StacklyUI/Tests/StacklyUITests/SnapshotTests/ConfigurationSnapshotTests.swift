import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Configuration tab, the Rename sheet, the Ports sheet, and the drift item of Needs
/// Attention (spec 0029 AC-15, AC-16, AC-18, AC-19; spec 0068): PostgreSQL, NATS, and Traefik
/// from their real keys and classes, a pending change, an invalid one, Effective File, the
/// drift and roll back banners, and the daemon offline.
extension SnapshotTests {
    struct ConfigurationSnapshotTests {
        private static let canvas = SnapshotSize.fitting(width: 1_044, narrowWidth: 640)
        private static let sheet = SnapshotSize.fitting(width: 520, narrowWidth: 480)
        private static let summary = InstanceSummary.fixture(name: "pg17-dev")

        /// The tab of one model with the padding of the page, on a canvas as tall as the live
        /// tab.
        @MainActor
        private func tab(
            _ model: ConfigurationModel, height: CGFloat = 1_100, offline: Bool = false
        ) -> some View {
            ScrollView {
                ConfigurationTab(
                    model: model, summary: Self.summary,
                    context: InstanceActionContext(report: { _ in })
                )
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.vertical, Spacing.s16)
            }
            .frame(height: height)
            .background(StacklyColor.surfaceWindow)
            .environment(
                \.mutationAvailability,
                offline
                    ? .blocked(reason: "stacklyd is unavailable", since: FixtureClock.now)
                    : .available
            )
            .actionHost()
        }

        @Test(
            .evidence(
                covers: ["REQ-015", "VAL-012"],
                expected:
                    "PostgreSQL in sync: Revision 3 In sync, a locked secret and set once field, a restart and a reload field with their notes, Ports locked, and History"
            ))
        func postgresqlInSync() async throws {
            let model = await FixtureConfiguration.model(.postgresql)
            try await assertSnapshot(
                tab(model), named: FixtureConfiguration.Case.postgresql.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "A restart field and a reload field changed: Pending Changes lists key = old and key = new, the tag says Requires restart, and Apply says Apply and Restart"
            ))
        func postgresqlWithAPendingRestart() async throws {
            let model = await FixtureConfiguration.model(.postgresql, edit: .restartAndReload)
            try await assertSnapshot(
                tab(model, height: 1_250), named: "config-postgresql-pending-restart",
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected: "A reload field alone says Applies on reload and Apply and Reload"
            ))
        func postgresqlWithAPendingReload() async throws {
            let model = await FixtureConfiguration.model(.postgresql, edit: .reloadOnly)
            try await assertSnapshot(
                tab(model, height: 1_250), named: "config-postgresql-pending-reload",
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["AC-015"],
                expected:
                    "A number outside its bounds shows the error under its field, Pending Changes stays empty, and Apply is off"
            ))
        func postgresqlWithAnInvalidValue() async throws {
            let model = await FixtureConfiguration.model(.postgresql, edit: .invalid)
            try await assertSnapshot(
                tab(model, height: 1_250), named: "config-postgresql-invalid", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "NATS cannot reload, so every editable field says Requires restart, and the managed field is locked"
            ))
        func natsRequiresARestartForEveryField() async throws {
            let model = await FixtureConfiguration.model(.nats)
            try await assertSnapshot(
                tab(model, height: 900), named: FixtureConfiguration.Case.nats.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "Traefik routes show as a list with one row per route and Add, from the schema alone"
            ))
        func traefikRoutesAreAList() async throws {
            let model = await FixtureConfiguration.model(.traefik)
            try await assertSnapshot(
                tab(model, height: 1_000), named: FixtureConfiguration.Case.traefik.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "A route added to the list is a pending change keyed by its name and applies on reload"
            ))
        func traefikWithARouteAdded() async throws {
            let model = await FixtureConfiguration.model(.traefik, edit: .addRoute)
            try await assertSnapshot(
                tab(model, height: 1_300), named: "config-traefik-route-added", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "A file changed outside Stackly shows the drift banner with Keep Revision and Show Diff, and Apply is off"
            ))
        func aFileChangedOutsideStackly() async throws {
            let model = await FixtureConfiguration.model(.drift, edit: .reloadOnly)
            try await assertSnapshot(
                tab(model, height: 1_250), named: FixtureConfiguration.Case.drift.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "A revision that never reached ready shows the roll back banner naming the last known good revision"
            ))
        func aRevisionThatNeverReachedReady() async throws {
            let model = await FixtureConfiguration.model(.rollBack)
            try await assertSnapshot(
                tab(model, height: 1_150), named: FixtureConfiguration.Case.rollBack.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected: "With the daemon offline the pending change stays and Apply is off"
            ))
        func theDaemonIsOffline() async throws {
            let model = await FixtureConfiguration.model(.postgresql, edit: .restartAndReload)
            try await assertSnapshot(
                tab(model, height: 1_250, offline: true), named: "config-offline",
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "Effective File shows the preview with the changed lines marked, read only"
            ))
        func effectiveFileMarksTheChangedLines() async throws {
            let model = await FixtureConfiguration.model(.postgresql, edit: .restartAndReload)
            model.mode = .effectiveFile
            try await assertSnapshot(
                tab(model, height: 420), named: "config-effective-file", size: Self.canvas)
        }

        // MARK: - Sheets

        @Test(
            .evidence(
                covers: ["BR-003"],
                expected:
                    "Rename shows the live check: a name another instance has reads as taken and the button is off"
            ))
        func renameWithATakenName() async throws {
            let model = FixtureConfiguration.renameModel(typing: "billing-db")
            try await assertSnapshot(
                RenameSheet(model: model, context: InstanceActionContext(report: { _ in })),
                named: "rename-sheet-taken", size: Self.sheet)
        }

        @Test(
            .evidence(
                covers: ["BR-003"],
                expected: "Rename with a free name has Rename on"
            ))
        func renameWithAFreeName() async throws {
            let model = FixtureConfiguration.renameModel(typing: "pg17-main")
            try await assertSnapshot(
                RenameSheet(model: model, context: InstanceActionContext(report: { _ in })),
                named: "rename-sheet", size: Self.sheet)
        }

        @Test(
            .evidence(
                covers: ["REQ-010", "AC-056"],
                expected:
                    "A taken fixed port reads as used by another instance, offers the free port, and never takes it on its own"
            ))
        func portsWithAConflict() async throws {
            let model = await FixtureConfiguration.endpointsModel(typing: "5433", taken: true)
            try await assertSnapshot(
                EndpointsSheet(model: model, context: InstanceActionContext(report: { _ in })),
                named: "ports-sheet-conflict", size: Self.sheet)
        }

        @Test(
            .evidence(
                covers: ["REQ-010"],
                expected: "A free port says the instance restarts briefly and Change Ports is on"
            ))
        func portsThatCanChange() async throws {
            let model = await FixtureConfiguration.endpointsModel(typing: "5440")
            try await assertSnapshot(
                EndpointsSheet(model: model, context: InstanceActionContext(report: { _ in })),
                named: "ports-sheet", size: Self.sheet)
        }

        // MARK: - Inside the pages

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "The detail page opens on Configuration under the Summary, Configuration, Logs, and Metrics picker"
            ))
        func theDetailPageShowsTheConfigurationTab() async throws {
            let rig = PageRig(instances: FixtureScenario.instanceCases)
            await rig.loadInstances()
            try await assertSnapshot(
                rig.instancesPage(
                    opening: FixtureConfiguration.instance, openingTab: .configuration,
                    showsInspector: false
                )
                .frame(height: 1_300),
                named: "instance-detail-configuration", size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-015"],
                expected:
                    "An instance with a config drift blocker is an item of Needs Attention that opens Configuration"
            ))
        func theOverviewShowsTheDriftItem() async throws {
            let drift = InstanceSummary.fixture(
                number: 9, name: "orders-db", lifecycle: .stopped,
                blocker: .configDrift(
                    ConfigDriftBlocker(
                        files: ["postgresql.conf"], observedAt: FixtureClock.minutesAgo(4))))
            let rig = PageRig(
                instances: [InstanceSummary.fixture(number: 1), drift], operations: [])
            await rig.load()
            try await assertSnapshot(rig.overviewPage(), named: "overview-config-drift")
        }
    }
}
