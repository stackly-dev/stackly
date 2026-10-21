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
                covers: ["REQ-024", "REQ-027"],
                expected: "The unbuilt page placeholder of Overview renders in every variant"
            ))
        func unbuiltOverviewPage() async throws {
            try await assertSnapshot(UnbuiltPage(section: .overview), named: "unbuilt-overview")
        }
    }
}
