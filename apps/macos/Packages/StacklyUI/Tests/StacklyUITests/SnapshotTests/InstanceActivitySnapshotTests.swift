import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Activity tab in each state of spec 0035 AC-19, from ``FixtureActivity``: a row of every
/// action, the empty tab, a first load that failed, and an older daemon; and the tab inside the
/// detail page of `pg17-dev` under the tab picker.
extension SnapshotTests {
    struct InstanceActivitySnapshotTests {
        private static let canvas = SnapshotSize.fitting(width: 1_044, narrowWidth: 640)

        /// The tab of one fixture with the page's padding.
        @MainActor
        private static func tab(_ fixture: FixtureActivity.Case, height: CGFloat = 760) async
            -> some View
        {
            let model = await FixtureActivity.model(fixture)
            return VStack(alignment: .leading) {
                InstanceActivityTab(model: model, operations: nil, completedRounds: 0)
            }
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s16)
            .frame(height: height)
            .background(StacklyColor.surfaceWindow)
        }

        @Test(
            .evidence(
                covers: ["REQ-045", "NFR-012"],
                expected:
                    "the Activity table shows Time, By, and Activity for every action, a failure with its code, and no secret value"
            ))
        @MainActor
        func theTabShowsARowOfEveryAction() async throws {
            try await assertSnapshot(
                await Self.tab(.rows), named: FixtureActivity.Case.rows.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-045", "NFR-012"],
                expected: "an instance without rows says No Activity Yet"
            ))
        @MainActor
        func anInstanceWithoutRowsSaysSo() async throws {
            try await assertSnapshot(
                await Self.tab(.empty, height: 360), named: FixtureActivity.Case.empty.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-045", "NFR-012"],
                expected: "a first load that failed shows Couldn’t Load Activity with Retry"))
        @MainActor
        func aFailedLoadOffersRetry() async throws {
            try await assertSnapshot(
                await Self.tab(.failed, height: 360), named: FixtureActivity.Case.failed.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-045", "NFR-012"],
                expected: "an older daemon without audit.list shows Activity Unavailable"))
        @MainActor
        func anOlderDaemonShowsActivityUnavailable() async throws {
            try await assertSnapshot(
                await Self.tab(.unsupported, height: 360),
                named: FixtureActivity.Case.unsupported.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-045", "NFR-012"],
                expected:
                    "the detail page opens on the Activity tab after Metrics in the tab picker"))
        @MainActor
        func theDetailPageShowsTheActivityTab() async throws {
            let rig = PageRig(instances: FixtureScenario.instanceCases)
            await rig.loadInstances()
            try await assertSnapshot(
                rig.instancesPage(
                    opening: FixtureActivity.instance, openingTab: .activity,
                    showsInspector: false
                )
                .frame(height: 900),
                named: "instance-detail-activity", size: Self.canvas)
        }
    }
}
