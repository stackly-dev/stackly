import SwiftUI
import Testing

@testable import StacklyUI

/// The shell's own pieces in every variant (spec 0068, AC-8): the banner and footer of
/// each daemon presence, and the placeholder of a page that is not built yet.
extension SnapshotTests {
    struct ShellSnapshotTests {
        @Test(
            .evidence(
                covers: ["REQ-024", "NFR-012"],
                expected:
                    "The daemon status matrix renders every presence's footer and banner in every variant"
            ))
        func daemonStatusMatrix() async throws {
            try await assertSnapshot(
                DaemonStatusMatrix(), named: "daemon-status-matrix",
                size: .fitting(width: 1044, narrowWidth: 640))
        }

        @Test(
            .evidence(
                covers: ["REQ-014", "REQ-027"],
                expected:
                    "The agent banner matrix renders every state's window banner in every variant"
            ))
        func agentBannerMatrix() async throws {
            #expect(
                AgentBannerMatrix.rows.allSatisfy {
                    AgentBanner.content($0.input, now: FixtureClock.now) != nil
                }, "every row of the matrix must have a banner")
            try await assertSnapshot(
                AgentBannerMatrix(), named: "agent-banner-matrix",
                size: .fitting(width: 1044, narrowWidth: 640))
        }

        @Test(
            .evidence(
                covers: ["REQ-024", "REQ-027"],
                expected: "The unbuilt page placeholder of Overview renders in every variant"
            ))
        func unbuiltOverviewPage() async throws {
            try await assertSnapshot(UnbuiltPage(section: .overview), named: "unbuilt-overview")
        }
    }
}
