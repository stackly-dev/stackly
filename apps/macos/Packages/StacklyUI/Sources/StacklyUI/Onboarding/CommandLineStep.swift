import SwiftUI

/// The Command Line step: its header, the row of the `stackly` command, and a block of
/// example commands (spec 0016, AC-8).
///
/// The state is the `CommandLineToolModel` of the runtime, the same one Diagnostics reads. The
/// switch and Repair… only ask for a confirmation; only the action button of that confirmation
/// changes a file. Without the model the row and the PATH note are left out and the examples
/// stay.
struct CommandLineStep: View {
    let focus: AccessibilityFocusState<Bool>.Binding

    @Environment(CommandLineToolModel.self) private var model: CommandLineToolModel?

    var body: some View {
        VStack(spacing: Spacing.s14) {
            OnboardingStepHeader(
                title: OnboardingCopy.commandLineTitle,
                subtitle: OnboardingCopy.commandLineSubtitle, focus: focus)
            if let model {
                CommandLineRow(model: model)
            }
            ConsoleBlock(
                text: OnboardingCopy.commandExamplesText,
                accessibilityLabel: OnboardingCopy.commandExamplesLabel)
        }
    }

    /// The row over the model, with its confirmation and a status read when it appears.
    private struct CommandLineRow: View {
        @Bindable var model: CommandLineToolModel

        var body: some View {
            CommandLineCard(
                content: CommandLineStepContent.make(
                    status: model.status.command, bundledTool: model.bundledTool,
                    lastError: model.lastError, isWorking: model.isWorking),
                toggle: { model.requestToggle($0) },
                repair: { model.requestRepair() }
            )
            .stacklyConfirmation(item: $model.pendingPrompt)
            .task { await model.refresh() }
        }
    }
}

extension CommandLineToolModel {
    /// The switch of the row was turned: on asks to install, off asks to remove. Both only
    /// fill ``pendingPrompt``; the switch itself never changes, it follows ``status``.
    func requestToggle(_ isOn: Bool) {
        if isOn {
            requestInstall()
        } else {
            requestRemove()
        }
    }
}
