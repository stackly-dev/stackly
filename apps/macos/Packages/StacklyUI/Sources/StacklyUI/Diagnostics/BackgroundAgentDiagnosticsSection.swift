import SwiftUI

/// The Background Agent section of Diagnostics, under the Connection section: a card with
/// the agent's state and what to do about it, and the actions that fit the state (spec
/// 0015, *GUI*). It shows only when a ``DaemonModel`` with an agent is in the environment.
struct BackgroundAgentDiagnosticsSection: View {
    @Environment(DaemonModel.self) private var model: DaemonModel?
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    var body: some View {
        if let model, let agent = model.agent {
            content(model: model, agent: agent)
        }
    }

    private func content(model: DaemonModel, agent: BackgroundAgentModel) -> some View {
        let facts = AgentDiagnostics(
            assessment: agent.assessment, bundlePath: agent.bundlePath,
            lastLaunchedAt: agent.lastLaunchedAt, lastCheckedAt: agent.lastCheckedAt,
            lastError: agent.lastError, detail: agent.unavailableDetail)
        let rows = facts.rows(calendar: calendar, locale: locale, timeZone: timeZone)
        let actions = Self.actions(
            for: agent.assessment?.state, daemonIsUnreachable: model.presence.isUnreachableForAgent)
        return VStack(alignment: .leading, spacing: Spacing.s8) {
            Text("Background Agent")
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            DiagnosticsCard(rows: rows)
            HStack(spacing: Spacing.s8) {
                ForEach(actions, id: \.self) { action in
                    Button(action.title) { perform(action, agent: agent) }
                        .disabled(agent.isWorking)
                }
                Button("Check Again") { Task { await agent.refresh() } }
                    .disabled(agent.isWorking)
                Button("Copy Details") {
                    PlainPasteboard.write(ConnectionDiagnostics.copyText(rows))
                }
            }
            .buttonStyle(.bordered)
            .padding(.top, Spacing.s4)
        }
    }

    /// The actions that fit `state`, as the banner offers them. Start stacklyd shows for an
    /// enabled agent only while the daemon is unreachable; Check Again and Copy Details are
    /// always there.
    static func actions(
        for state: BackgroundAgentState?, daemonIsUnreachable: Bool
    ) -> [DaemonBannerAction] {
        switch state {
        case .notRegistered: [.turnOn]
        case .requiresApproval, .disabled: [.openLoginItems]
        case .enabled: daemonIsUnreachable ? [.startDaemon] : []
        case .notFound, .unavailable, nil: []
        }
    }

    private func perform(_ action: DaemonBannerAction, agent: BackgroundAgentModel) {
        switch action {
        case .turnOn: Task { await agent.turnOn() }
        case .startDaemon: Task { await agent.start() }
        case .openLoginItems: agent.openLoginItems()
        case .checkAgain: Task { await agent.refresh() }
        case .reconnect, .openDiagnostics: break
        }
    }
}

extension DaemonPresence {
    /// Whether the presence is `unreachable`, for the agent actions.
    fileprivate var isUnreachableForAgent: Bool {
        if case .unreachable = self { return true }
        return false
    }
}
