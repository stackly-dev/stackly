import SwiftUI

/// The row of the Command Line step: the title and detail, the control, the PATH note of an
/// installed command, and the last error (spec 0016, AC-8). It takes only the value
/// ``CommandLineStepContent/make(status:bundledTool:lastError:isWorking:)`` built, so a
/// snapshot shows any row without a model.
struct CommandLineCard: View {
    let content: CommandLineStepContent
    /// Called when the switch is turned: `true` asks to install, `false` to remove.
    var toggle: (Bool) -> Void = { _ in }
    /// Called when Repair… is pressed.
    var repair: () -> Void = {}

    @Environment(\.loadingDelay) private var loadingDelay

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            HStack(alignment: .top, spacing: Spacing.s12) {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(content.title)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(.primary)
                    Text(content.detail)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: Spacing.s12)
                if content.isWorking {
                    DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
                        .frame(width: 24, height: 24)
                }
                control
            }
            .padding(Spacing.cardInsets)
            .cardSurface()
            if content.showsPathNote {
                pathNote
            }
            if let error = content.lastError {
                Label {
                    Text(error)
                        .font(.callout)
                        .foregroundStyle(Tone.critical.text)
                        .fixedSize(horizontal: false, vertical: true)
                } icon: {
                    Image(systemName: Tone.critical.symbol)
                        .foregroundStyle(Tone.critical.fill)

                }
                .padding(.horizontal, Spacing.s4)
            }
        }
    }

    @ViewBuilder private var control: some View {
        switch content.control {
        case .toggle(let isOn, let reason):
            Toggle(
                content.title,
                isOn: Binding(get: { isOn }, set: { toggle($0) })
            )
            .labelsHidden()
            .toggleStyle(.switch)
            .disabled(reason: reason)
        case .repair(let reason):
            Button(OnboardingCopy.commandLineRepair) { repair() }
                .buttonStyle(.bordered)
                .disabled(reason: reason)
        }
    }

    private var pathNote: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            Text(OnboardingCopy.commandLinePathNote)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: Spacing.s8) {
                Text(OnboardingCopy.commandLinePathLine)
                    .stacklyCode()
                CopyButton(
                    OnboardingCopy.commandLinePathLine, what: OnboardingCopy.commandLinePathWhat,
                    style: .text)
            }
            if let path = content.runDirectlyPath {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(OnboardingCopy.commandLineRunDirectly)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    Text(path)
                        .stacklyCode()
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .padding(.horizontal, Spacing.s4)
    }
}
