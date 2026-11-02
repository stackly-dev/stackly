import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Metrics tab in each of its states (spec 0028 AC-13, *Layar Metrics*; spec 0068): the six
/// fixtures of ``FixtureMetrics`` on their own, the tab inside the detail page of a running and
/// of a stopped instance, and the Restarts and latency rows of Summary and the inspector.
extension SnapshotTests {
    struct InstanceMetricsSnapshotTests {
        private static let canvas = SnapshotSize.fitting(width: 1_044, narrowWidth: 640)
        private static let instance = FixtureID.uuid(kind: FixtureID.instance, number: 1)
        private static let stopped = FixtureID.uuid(kind: FixtureID.instance, number: 4)

        /// The tab of one fixture with the page's padding, on a canvas as tall as the live tab.
        @MainActor
        private func tab(_ fixture: FixtureMetrics.Case, height: CGFloat = 1_000) -> some View {
            ScrollView {
                InstanceMetricsTab(
                    model: FixtureMetrics.model(fixture, instance: Self.instance),
                    instanceName: "pg17-dev",
                    action: StacklyAction(
                        id: InstanceVerb.start.rawValue, title: "Start", mutates: true
                    ) {},
                    completedRounds: 0
                )
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.vertical, Spacing.s16)
            }
            .frame(height: height)
            .background(StacklyColor.surfaceWindow)
        }

        @Test(
            .evidence(
                covers: ["REQ-018", "AC-018"],
                expected:
                    "The live tab shows Main Process, the health line, five cards, the CPU graph, and the processes"
            ))
        func theLiveTabShowsTheMainProcess() async throws {
            try await assertSnapshot(
                tab(.liveMain), named: FixtureMetrics.Case.liveMain.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-018", "AC-018"],
                expected:
                    "Process Tree shows the summed cards with the process count and a CPU over 100"
            ))
        func theTreeScopeShowsTheSummedCards() async throws {
            try await assertSnapshot(
                tab(.liveTree), named: FixtureMetrics.Case.liveTree.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["AC-018"],
                expected:
                    "A tree where 5 of 6 processes were measured says so, and a partial scan says at least"
            ))
        func aPartlyMeasuredTreeSaysHowManyWereMeasured() async throws {
            try await assertSnapshot(
                tab(.partial), named: FixtureMetrics.Case.partial.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-018", "AC-018"],
                expected:
                    "Every missing number reads Unavailable with the text of its reason, never zero"
            ))
        func missingNumbersReadUnavailableWithTheirReason() async throws {
            try await assertSnapshot(
                tab(.unavailable, height: 760),
                named: FixtureMetrics.Case.unavailable.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-018"],
                expected: "A process list cut at its limit ends with how many more are not shown"))
        func aTruncatedProcessListSaysHowManyAreNotShown() async throws {
            try await assertSnapshot(
                tab(.truncated), named: FixtureMetrics.Case.truncated.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-018"],
                expected: "A stopped instance shows No Live Metrics with an enabled Start"))
        func aStoppedInstanceShowsNoLiveMetrics() async throws {
            let rig = PageRig(instances: FixtureScenario.instanceCases)
            await rig.loadInstances()
            try await assertSnapshot(
                rig.instancesPage(
                    opening: Self.stopped, openingTab: .metrics, metrics: .notRunning,
                    showsInspector: false
                )
                .frame(height: 720),
                named: FixtureMetrics.Case.notRunning.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-018"],
                expected:
                    "The detail page opens on the Metrics tab under the Summary, Logs, and Metrics picker"
            ))
        func theDetailPageShowsTheMetricsTab() async throws {
            let rig = PageRig(instances: FixtureScenario.instanceCases)
            await rig.loadInstances()
            try await assertSnapshot(
                rig.instancesPage(
                    opening: Self.instance, openingTab: .metrics, showsInspector: false
                )
                .frame(height: 1_300),
                named: "instance-detail-metrics", size: Self.canvas)
        }
    }
}
