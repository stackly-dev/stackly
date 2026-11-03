import StacklyClient
import SwiftUI

/// The Startup group of the General tab (spec 0032 AC-12, AC-15): Start stacklyd at login,
/// which is the background agent of spec 0015, and Open Stackly at login, which is the app
/// itself. The two are separate: turning one off touches neither the other nor any instance.
struct StartupSettingsSection: View {
    let model: DaemonModel
    let loginItem: LoginItemModel

    private var atLoginCount: Int {
        model.instances.store.items.values.filter { $0.startup.policy == .atLogin }.count
    }

    var body: some View {
        Section("Startup") {
            if SettingsAvailability.isShown(.agentStartup), let agent = model.agent {
                AgentStartupRow(model: model, agent: agent, atLoginCount: atLoginCount)
            }
            if SettingsAvailability.isShown(.openAtLogin) {
                OpenAtLoginRow(loginItem: loginItem)
            }
        }
        .task { await loginItem.refresh() }
    }
}

/// The Start stacklyd at login row.
private struct AgentStartupRow: View {
    let model: DaemonModel
    let agent: BackgroundAgentModel
    let atLoginCount: Int
    @State private var confirmation: ConfirmationRequest?

    private var turnOff: TurnOffModel { model.turnOff }

    var body: some View {
        let content = StartupRowContent.make(
            assessment: agent.assessment, atLoginCount: atLoginCount, isWorking: agent.isWorking,
            isTurningOff: turnOff.phase != .idle)
        LabeledContent {
            HStack(spacing: Spacing.s8) {
                if agent.isWorking {
                    ProgressView().controlSize(.small)
                }
                if content.showsOpenLoginItems {
                    ActionButton(
                        StacklyAction(
                            id: "settings.agent.openLoginItems", title: "Open Login Items",
                            mutates: false, perform: { agent.openLoginItems() }),
                        showsReason: false)
                }
                Toggle(
                    "Start stacklyd at login",
                    isOn: Binding(
                        get: { content.isOn },
                        set: { on in
                            if on {
                                Task { await agent.turnOn() }
                            } else {
                                Task { await turnOff.begin(from: .settings) }
                            }
                        })
                )
                .labelsHidden()
                .disabled(content.toggleDisabledReason != nil)
                .help(content.toggleDisabledReason ?? "")
            }
        } label: {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text("Start stacklyd at login")
                Text(content.stateText)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                if let note = content.note {
                    Text(note)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let reason = content.toggleDisabledReason, agent.isWorking == false,
                    turnOff.phase == .idle
                {
                    DisabledReasonText(reason)
                }
                if let message = turnOff.message {
                    Text(message)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .onChange(of: turnOff.plan) { _, plan in
            confirmation = (turnOff.origin == .settings ? plan : nil).map { plan in
                ConfirmationRequest(
                    title: plan.title, message: plan.message, actionTitle: "Turn Off",
                    role: .destructive,
                    perform: { Task { await turnOff.confirm() } })
            }
        }
        .stacklyConfirmation(item: $confirmation)
    }
}

/// The Open Stackly at login row.
private struct OpenAtLoginRow: View {
    let loginItem: LoginItemModel

    var body: some View {
        LabeledContent {
            HStack(spacing: Spacing.s8) {
                if loginItem.isWorking {
                    ProgressView().controlSize(.small)
                }
                if loginItem.needsApproval {
                    ActionButton(
                        StacklyAction(
                            id: "settings.openAtLogin.openLoginItems", title: "Open Login Items",
                            mutates: false, perform: { loginItem.openLoginItems() }),
                        showsReason: false)
                }
                Toggle(
                    "Open Stackly at login",
                    isOn: Binding(
                        get: { loginItem.isOn },
                        set: { on in Task { await loginItem.setEnabled(on) } })
                )
                .labelsHidden()
                .disabled(loginItem.isWorking || loginItem.status == nil)
            }
        } label: {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text("Open Stackly at login")
                Text(StartupCopy.openAtLoginNote)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                if loginItem.needsApproval {
                    Text(StartupCopy.openAtLoginApproval)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
                if let error = loginItem.error {
                    Text(error)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }
}
