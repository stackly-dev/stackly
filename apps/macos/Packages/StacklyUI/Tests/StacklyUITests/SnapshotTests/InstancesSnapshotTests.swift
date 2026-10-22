import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Instances page in every variant (spec 0068 AC-8, spec 0022 AC-13): the instance
/// that is chosen in each of its shapes, the inspector open and closed, and every state of
/// the page.
///
/// The states with the inspector open render at the width of the page column and the
/// inspector together: 664 pt is the content column of a 900 pt window, which leaves the
/// 384 pt of the table (spec 0022, *Halaman Instances*).
extension SnapshotTests {
    struct InstancesSnapshotTests {
        /// The page these snapshots show, named so the source rule finds its suite.
        private typealias Page = InstancesPage

        private static func id(_ number: UInt8) -> String {
            FixtureID.uuid(kind: FixtureID.instance, number: number).uuidString.lowercased()
        }

        /// The page area with the inspector open, narrow at the 900 pt window.
        private static let withInspector = SnapshotSize.fitting(width: 1_044, narrowWidth: 664)

        @MainActor
        private func rig(
            instances: [InstanceSummary] = FixtureScenario.instanceCases,
            load: Bool = true, prepare: (PageRig) -> Void = { _ in }
        ) async -> PageRig {
            let rig = PageRig(instances: instances)
            prepare(rig)
            if load { await rig.loadInstances() }
            return rig
        }

        /// A view of the page area's height, so `fitting` knows how tall it is.
        @MainActor
        private func framed<V: View>(_ view: V) -> some View {
            view.frame(height: 748)
        }

        // MARK: - Chosen instance, inspector open

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page with a ready instance chosen renders in every variant"
            ))
        func instancesWithAReadyInstanceChosen() async throws {
            let rig = await rig()
            try await assertSnapshot(
                framed(rig.instancesPage(selection: Self.id(1))), named: "instances-ready",
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Instances page with a failed instance chosen renders in every variant"
            ))
        func instancesWithAFailedInstanceChosen() async throws {
            let rig = await rig()
            try await assertSnapshot(
                framed(rig.instancesPage(selection: Self.id(5))), named: "instances-failed-chosen",
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Instances page with a blocked instance chosen renders in every variant"
            ))
        func instancesWithABlockedInstanceChosen() async throws {
            let rig = await rig()
            try await assertSnapshot(
                framed(rig.instancesPage(selection: Self.id(7))), named: "instances-blocked",
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Instances page with an instance that is not ready chosen renders in every variant"
            ))
        func instancesWithAnInstanceThatIsNotReadyChosen() async throws {
            let rig = await rig()
            try await assertSnapshot(
                framed(rig.instancesPage(selection: Self.id(2))), named: "instances-not-ready",
                size: Self.withInspector)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page with nothing chosen renders in every variant"
            ))
        func instancesWithNothingChosen() async throws {
            let rig = await rig()
            try await assertSnapshot(
                framed(rig.instancesPage()), named: "instances-no-selection",
                size: Self.withInspector)
        }

        // MARK: - Inspector closed

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page with the inspector closed renders in every variant"
            ))
        func instancesWithTheInspectorClosed() async throws {
            let rig = await rig()
            try await assertSnapshot(
                rig.instancesPage(selection: Self.id(1), showsInspector: false),
                named: "instances-inspector-closed")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page sorted by status renders in every variant"
            ))
        func instancesSortedByStatus() async throws {
            let rig = await rig()
            try await assertSnapshot(
                rig.instancesPage(
                    showsInspector: false, sortOrder: InstanceSortComparator(.status)),
                named: "instances-sorted-status")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page filtered to Failed renders in every variant"
            ))
        func instancesFilteredToFailed() async throws {
            let rig = await rig()
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false, filter: .failed),
                named: "instances-filter-failed")
        }

        // MARK: - States of the page

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page for a search with no results renders in every variant"
            ))
        func instancesWithNoResults() async throws {
            let rig = await rig()
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false, query: "zzz"),
                named: "instances-no-results")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page without instances renders in every variant"
            ))
        func instancesWithoutInstances() async throws {
            let rig = await rig(instances: [])
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false), named: "instances-empty")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page before the first snapshot renders in every variant"
            ))
        func instancesBeforeTheFirstSnapshot() async throws {
            let rig = await rig(load: false)
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false), named: "instances-loading",
                contrastInvariant: true,
                contrastInvariantReason:
                    "only a system spinner, which Increase Contrast does not change")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page that failed to load renders in every variant"
            ))
        func instancesThatFailedToLoad() async throws {
            let rig = await rig(load: false) {
                $0.script.instanceFailure = .rejected(
                    ErrorBody(
                        code: "internal.unexpected", category: .internal,
                        message: "the registry could not be read", retryable: false,
                        resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
                        rule: nil, items: nil))
            }
            await rig.loadInstances()
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false), named: "instances-failed")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Instances page while the daemon cannot be reached renders in every variant"
            ))
        func instancesWhileWaitingForTheDaemon() async throws {
            let rig = await rig(load: false)
            try await assertSnapshot(
                rig.instancesPage(
                    waiting: FailureContent(
                        title: "Waiting for stacklyd",
                        message: "Stackly shows this page once the daemon is reachable.",
                        code: "ipc.endpoint_missing"),
                    hasConnection: false, showsInspector: false),
                named: "instances-waiting")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Instances page of a daemon that cannot list instances renders in every variant"
            ))
        func instancesOfADaemonWithoutTheMethod() async throws {
            let rig = await rig(load: false) {
                $0.script.instanceFailure = .rejected(PageRig.unknownMethod)
            }
            await rig.loadInstances()
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false), named: "instances-daemon-unsupported")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Instances page after a failed refresh renders in every variant"
            ))
        func instancesAfterAFailedRefresh() async throws {
            let rig = await rig()
            rig.script.instanceFailure = .rejected(PageRig.failure)
            await rig.instances.store.refresh()
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false), named: "instances-stale")
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Instances page with an instance that could not be refreshed renders in every variant"
            ))
        func instancesWithAnInstanceThatCouldNotBeRefreshed() async throws {
            let rig = await rig()
            // A health event for an instance the store does not hold asks the daemon for it,
            // and the daemon fails.
            rig.script.oneFailure = .rejected(PageRig.failure)
            let stranger = InstanceSummary.fixture(number: 40, name: "newcomer")
            rig.instances.store.receive(Self.healthEvent(for: stranger))
            try await waitUntil { rig.instances.store.itemFailures.isEmpty == false }
            try await assertSnapshot(
                rig.instancesPage(showsInspector: false), named: "instances-partial")
        }

        private static func healthEvent(for instance: InstanceSummary) -> Event {
            Event(
                subscriptionId: 1, sequence: 1, at: FixtureClock.now,
                resource: ResourceRef(kind: .instance, id: instance.id.uuidString.lowercased()),
                revision: 9,
                kind: .instanceHealthChanged(
                    InstanceHealthChanged(
                        instanceId: instance.id, statusSeq: 50, readiness: instance.readiness,
                        health: instance.health, blocker: nil, restart: instance.restart,
                        process: nil, logsUnavailable: false)))
        }
    }
}
