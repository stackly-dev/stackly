import SwiftUI

/// The Background Agent section of Diagnostics, under the Connection section: a card with
/// the agent's state and what to do about it, and the actions that fit the state (spec
/// 0015, *GUI*). It shows only when a ``DaemonModel`` with an agent is in the environment.
struct BackgroundAgentDiagnosticsSection: View {
    @Environment(DaemonModel.self) private var model: DaemonModel?
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    @State private var confirmation: ConfirmationRequest?
    @State private var startFacts = AgentStartFacts.none

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
        let rows = Self.rows(
            facts.rows(calendar: calendar, locale: locale, timeZone: timeZone),
            adding: startFacts, calendar: calendar, locale: locale, timeZone: timeZone)
        let actions = Self.actions(
            for: agent.assessment?.state, daemonIsUnreachable: model.presence.isUnreachableForAgent)
        return VStack(alignment: .leading, spacing: Spacing.s8) {
            Text("Background Agent")
                .font(.headline)

            DiagnosticsCard(rows: rows)
            HStack(spacing: Spacing.s8) {
                ForEach(actions, id: \.self) { action in
                    Button(action.title) { perform(action, agent: agent) }
                        .disabled(agent.isWorking)
                }
                if agent.assessment?.state == .enabled {
                    Button("Turn Off…") { Task { await model.turnOff.begin() } }
                        .disabled(agent.isWorking || model.turnOff.phase != .idle)
                }
                Button("Check Again") { Task { await agent.refresh() } }
                    .disabled(agent.isWorking)
                Button("Copy Details") {
                    PlainPasteboard.write(ConnectionDiagnostics.copyText(rows))
                }
            }
            .buttonStyle(.bordered)
            .padding(.top, Spacing.s4)
            if let message = model.turnOff.message {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
        }
        .onChange(of: model.turnOff.plan) { _, plan in
            confirmation = (model.turnOff.origin == .diagnostics ? plan : nil).map { plan in
                ConfirmationRequest(
                    title: plan.title, message: plan.message, actionTitle: "Turn Off",
                    role: .destructive,
                    perform: { Task { await model.turnOff.confirm() } })
            }
        }
        .stacklyConfirmation(item: $confirmation)
        .task(id: StartFactsKey(model: model, agent: agent)) {
            startFacts = await Self.readStartFacts(model: model, agent: agent)
        }
    }

    /// Reads the start failure record and the binary's change time, when the shell has
    /// readers and the daemon cannot be reached (spec 0034, AC-11).
    private static func readStartFacts(
        model: DaemonModel, agent: BackgroundAgentModel
    ) async -> AgentStartFacts {
        let unreachable = model.presence.isUnreachableForAgent
        guard let sources = model.startFacts, unreachable else { return .none }
        let record = await sources.record.read()
        let binary: Date? =
            if let path = agent.bundlePath {
                await sources.binary.modifiedAt(bundlePath: path)
            } else { nil }
        return AgentStartFacts.resolve(
            record: record, daemonUnreachable: unreachable,
            daemonStartedAt: model.lastDaemon?.info?.startedAt,
            agentEnabled: agent.assessment?.state == .enabled,
            lastLaunchedAt: agent.lastLaunchedAt, binaryModifiedAt: binary)
    }

    /// The agent's rows with what spec 0034 adds: What to do for `binary_replaced` after
    /// State, and Last start failure after Last error.
    static func rows(
        _ rows: [DiagnosticsRow], adding facts: AgentStartFacts, calendar: Calendar,
        locale: Locale, timeZone: TimeZone
    ) -> [DiagnosticsRow] {
        var rows = rows
        if facts.binaryReplaced, !rows.contains(where: { $0.label == "What to do" }) {
            let at = rows.firstIndex { $0.label == "State" }.map { $0 + 1 } ?? 0
            rows.insert(DiagnosticsRow("What to do", text: binaryReplacedStep), at: at)
        }
        if let record = facts.startFailure {
            let time = DiagnosticsTime.format(
                record.at, calendar: calendar, locale: locale, timeZone: timeZone)
            rows.append(
                DiagnosticsRow(
                    "Last start failure", code: record.code, text: "\(time) · \(record.message)"))
        }
        return rows
    }

    /// What to do when the binary changed after launchd last started it.
    static let binaryReplacedStep =
        "stacklyd in the app changed after launchd last started it. Start stacklyd to run the new one."

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

/// What the start facts of the Background Agent section depend on, so they are read again
/// when the daemon goes away or comes back and after Check Again.
private struct StartFactsKey: Equatable {
    let unreachable: Bool
    let lastLaunchedAt: Date?
    let lastCheckedAt: Date?
    let state: BackgroundAgentState?

    init(model: DaemonModel, agent: BackgroundAgentModel) {
        unreachable = model.presence.isUnreachableForAgent
        lastLaunchedAt = agent.lastLaunchedAt
        lastCheckedAt = agent.lastCheckedAt
        state = agent.assessment?.state
    }
}
