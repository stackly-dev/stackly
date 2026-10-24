import SwiftUI

/// The Command Line Tool section of Diagnostics (spec 0015, *GUI*): the state of the
/// `stackly` command in `~/.local/bin`, with Install, Repair, and Remove. It shows when a
/// ``CommandLineToolModel`` is in the environment. `// provisional: F32` (Settings gets its
/// own row).
struct CommandLineToolSection: View {
    @Environment(CommandLineToolModel.self) private var model: CommandLineToolModel?

    var body: some View {
        if let model {
            SectionContent(model: model)
        }
    }

    private struct SectionContent: View {
        @Bindable var model: CommandLineToolModel

        var body: some View {
            let facts = CommandLineToolDiagnostics(
                command: model.status.command, target: model.status.commandTarget,
                home: model.home.path)
            VStack(alignment: .leading, spacing: Spacing.s8) {
                Text("Command Line Tool")
                    .font(.headline)

                DiagnosticsCard(rows: facts.rows)
                HStack(spacing: Spacing.s8) {
                    Text(CommandLineToolDiagnostics.pathLine)
                        .stacklyCode()
                    CopyButton(
                        CommandLineToolDiagnostics.pathLine, what: "PATH line", style: .text)
                }
                .padding(.horizontal, Spacing.cardInsets.leading)
                if let error = model.lastError {
                    Text(error.message)
                        .font(.callout)
                        .foregroundStyle(Tone.critical.text)
                        .fixedSize(horizontal: false, vertical: true)
                }
                HStack(spacing: Spacing.s8) {
                    ForEach(facts.actions, id: \.self) { action in
                        button(action)
                    }
                }
                .buttonStyle(.bordered)
                .padding(.top, Spacing.s4)
                if model.canLink == false, facts.actions.contains(where: { $0 != .remove }) {
                    DisabledReasonText(CommandLineToolModel.missingToolReason)
                }
            }
            .stacklyConfirmation(item: $model.pendingPrompt)
            .task { await model.refresh() }
        }

        @ViewBuilder private func button(_ action: CommandLineToolDiagnostics.Action) -> some View {
            switch action {
            case .install:
                Button("Install") { model.requestInstall() }
                    .disabled(model.isWorking)
                    .disabled(reason: model.canLink ? nil : CommandLineToolModel.missingToolReason)
            case .repair:
                Button("Repair") { model.requestRepair() }
                    .disabled(model.isWorking)
                    .disabled(reason: model.canLink ? nil : CommandLineToolModel.missingToolReason)
            case .remove:
                Button("Remove") { model.requestRemove() }
                    .disabled(model.isWorking)
            }
        }
    }
}
