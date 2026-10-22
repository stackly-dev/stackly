import SwiftUI
import Testing

@testable import StacklyUI

/// Every row of the Background Agent card of the onboarding (spec 0016, AC-5), drawn from
/// values `BackgroundAgentStepContent.make` builds, so no model or runtime is involved.
extension SnapshotTests {
    struct OnboardingAgentSnapshotTests {
        private struct Row: Identifiable {
            let id: String
            let content: BackgroundAgentStepContent
        }

        private static let since = BackgroundAgentStepContentTests.since
        private static let footer = StatusAppearance(
            label: "stacklyd running", tone: .positive, symbol: Tone.positive.symbol)
        private static let down = StatusAppearance(
            label: "stacklyd unavailable", tone: .critical, symbol: Tone.critical.symbol)

        private static func make(
            _ state: BackgroundAgentState?, status: ShellStatus, code: String? = nil,
            reason: UnavailableReason? = nil, detail: String? = nil,
            after seconds: TimeInterval = 0,
            footer: StatusAppearance = down, lastError: AgentActionError? = nil,
            isWorking: Bool = false
        ) -> BackgroundAgentStepContent {
            BackgroundAgentStepContentTests.make(
                BackgroundAgentStepContentTests.input(
                    state, status: status, code: code, reason: reason, detail: detail),
                after: seconds, lastError: lastError, isWorking: isWorking, footer: footer)
        }

        private static var rows: [Row] {
            let missing = ShellStatus.unreachable(lastKnownAt: nil, since: since)
            let code = "ipc.endpoint_missing"
            return [
                Row(
                    id: "checking",
                    content: make(nil, status: .connecting(slow: false, since: since))),
                Row(
                    id: "connected",
                    content: make(.enabled, status: .connected(.ready), footer: footer)),
                Row(id: "starting", content: make(.enabled, status: missing, code: code)),
                Row(
                    id: "not-running",
                    content: make(.enabled, status: missing, code: code, after: 20)),
                Row(
                    id: "connecting",
                    content: make(.enabled, status: .connecting(slow: true, since: since))),
                Row(id: "other", content: make(.enabled, status: .stopping(since: since))),
                Row(id: "off", content: make(.notRegistered, status: missing, code: code)),
                Row(
                    id: "off-failed",
                    content: make(
                        .notRegistered, status: missing, code: code,
                        lastError: .daemonDidNotStart(seconds: 30))),
                Row(
                    id: "working",
                    content: make(.notRegistered, status: missing, code: code, isWorking: true)),
                Row(id: "approval", content: make(.requiresApproval, status: missing, code: code)),
                Row(id: "disabled", content: make(.disabled, status: missing, code: code)),
                Row(id: "not-found", content: make(.notFound, status: missing, code: code)),
                Row(
                    id: "no-helper",
                    content: make(.unavailable, status: .connected(.ready), reason: .helperMissing)),
                Row(
                    id: "root-override",
                    content: make(.unavailable, status: .connected(.ready), reason: .rootOverride)),
                Row(
                    id: "unreadable",
                    content: make(
                        .unavailable, status: .connected(.ready), reason: .statusFailed,
                        detail: "timed out")),
            ]
        }

        private struct Matrix: View {
            var body: some View {
                VStack(alignment: .leading, spacing: Spacing.s14) {
                    ForEach(OnboardingAgentSnapshotTests.rows) { row in
                        VStack(alignment: .leading, spacing: Spacing.s4) {
                            Text(row.id).font(.caption).foregroundStyle(StacklyColor.textSecondary)
                            BackgroundAgentCard(content: row.content)
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
                covers: ["REQ-014", "NFR-012"],
                expected: "Every row of the Background Agent card renders in every variant"
            ))
        func agentCardRows() async throws {
            try await assertSnapshot(
                Matrix(), named: "onboarding-agent-card-rows",
                size: .fitting(width: 600, narrowWidth: 600))
        }
    }
}
