import SwiftUI
import Testing

@testable import StacklyUI

/// Every row of the Command Line control of the onboarding (spec 0016, AC-8), drawn from
/// values `CommandLineStepContent.make` builds, so no model or home is involved.
extension SnapshotTests {
    struct OnboardingCommandLineSnapshotTests {
        private struct Row: Identifiable {
            let id: String
            let content: CommandLineStepContent
        }

        private static let tool = URL(
            filePath: "/Applications/Stackly.app/Contents/MacOS/stackly-cli")

        private static var rows: [Row] {
            func make(
                _ state: CommandLineLinkState, tool: URL? = tool,
                error: CommandLineToolError? = nil, isWorking: Bool = false
            ) -> CommandLineStepContent {
                CommandLineStepContent.make(
                    status: state, bundledTool: tool, lastError: error, isWorking: isWorking)
            }
            return [
                Row(id: "not-installed", content: make(.notInstalled)),
                Row(id: "installed", content: make(.installed)),
                Row(id: "stale", content: make(.stale)),
                Row(id: "broken", content: make(.broken)),
                Row(id: "conflict", content: make(.conflict)),
                Row(id: "no-command-not-installed", content: make(.notInstalled, tool: nil)),
                Row(id: "working", content: make(.installed, isWorking: true)),
                Row(
                    id: "failed",
                    content: make(.notInstalled, error: .folderNotSafe(path: "~/.local/bin"))),
            ]
        }

        private struct Matrix: View {
            var body: some View {
                VStack(alignment: .leading, spacing: Spacing.s14) {
                    ForEach(OnboardingCommandLineSnapshotTests.rows) { row in
                        VStack(alignment: .leading, spacing: Spacing.s4) {
                            Text(row.id).font(.caption).foregroundStyle(StacklyColor.textSecondary)
                            CommandLineCard(content: row.content)
                        }
                    }
                }
                // A spinner never shows in a reference: it would move.
                .environment(\.loadingDelay, .seconds(3600))
                .frame(width: OnboardingFlow.columnWidth)
                .padding(Spacing.pageHorizontal)
                .background(StacklyColor.surfaceWindow)
            }
        }

        @Test(
            .evidence(
                covers: ["REQ-032", "NFR-012"],
                expected: "Every row of the Command Line control renders in every variant"
            ))
        func commandLineRows() async throws {
            try await assertSnapshot(
                Matrix(), named: "onboarding-command-line-rows",
                size: .fitting(width: 600, narrowWidth: 600))
        }
    }
}
