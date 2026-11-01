import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Logs tab in each of its states (spec 0027 AC-11, *Layar Logs*; spec 0068): the five
/// fixtures of ``FixtureLogs`` on their own, and the tab inside the detail page.
extension SnapshotTests {
    struct InstanceLogsSnapshotTests {
        private static let canvas = SnapshotSize.fitting(width: 1_044, narrowWidth: 640)
        private static let instance = FixtureID.uuid(kind: FixtureID.instance, number: 1)

        /// The tab of one fixture with the page's padding, as tall as a window's content.
        @MainActor
        private func tab(_ fixture: FixtureLogs.Case) -> some View {
            InstanceLogsTab(
                model: FixtureLogs.model(fixture, instance: Self.instance), completedRounds: 0
            )
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s16)
            .frame(height: 560)
            .background(StacklyColor.surfaceWindow)
        }

        @Test(
            .evidence(
                covers: ["REQ-017"],
                expected:
                    "The Logs tab shows both streams with time, source, warning, and error, Follow on, and the retention footer"
            ))
        func theTabFollowsBothStreams() async throws {
            try await assertSnapshot(
                tab(.follow), named: FixtureLogs.Case.follow.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-017"],
                expected: "A store that holds nothing yet shows No output yet"))
        func anEmptyStoreShowsNoOutputYet() async throws {
            try await assertSnapshot(
                tab(.empty), named: FixtureLogs.Case.empty.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-017"],
                expected: "A filter with no matching line shows No lines match this filter."))
        func aFilterWithoutMatchesSaysSo() async throws {
            try await assertSnapshot(
                tab(.filterEmpty), named: FixtureLogs.Case.filterEmpty.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-017", "AC-017"],
                expected:
                    "Started, exited, truncated, unavailable, a rotated gap, and a day separator show as stackly rows"
            ))
        func markersShowAsStacklyRows() async throws {
            try await assertSnapshot(
                tab(.markers), named: FixtureLogs.Case.markers.rawValue, size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-017", "NFR-004"],
                expected: "A store the daemon cannot read shows Logs can’t be read with Try Again"))
        func anUnreadableStoreOffersTryAgain() async throws {
            try await assertSnapshot(
                tab(.unreadable), named: FixtureLogs.Case.unreadable.rawValue,
                size: Self.canvas)
        }

        @Test(
            .evidence(
                covers: ["REQ-017"],
                expected: "The detail page opens on the Logs tab under the Summary and Logs picker"
            ))
        func theDetailPageShowsTheLogsTab() async throws {
            let rig = PageRig(instances: FixtureScenario.instanceCases)
            await rig.loadInstances()
            try await assertSnapshot(
                rig.instancesPage(opening: Self.instance, openingTab: .logs, showsInspector: false)
                    .frame(height: 720),
                named: "instance-detail-logs", size: Self.canvas)
        }
    }
}
