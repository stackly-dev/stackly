import SwiftUI

/// The card of the Background Agent step: the icon, the title and detail of one row, the
/// daemon status of an enabled row, and the action on the right (spec 0016, AC-5). It takes
/// only the value `BackgroundAgentStepContent.make` built, so a preview or a snapshot shows
/// any row without a model.
struct BackgroundAgentCard: View {
    let content: BackgroundAgentStepContent
    /// Called when the action is pressed.
    var perform: (BackgroundAgentStepContent.Action) -> Void = { _ in }
    /// Where the action button takes keyboard focus from the step.
    var focus: FocusState<OnboardingFocusTarget?>.Binding?

    @Environment(\.loadingDelay) private var loadingDelay

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            HStack(alignment: .top, spacing: Spacing.s12) {
                icon
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(content.title)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(.primary)
                    if let detail = content.detail {
                        Text(detail)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    if let status = content.status {
                        StatusLabel(status, context: .row)
                            .font(.callout)
                            .padding(.top, Spacing.s2)
                    }
                }
                Spacer(minLength: Spacing.s12)
                trailing
            }
            .padding(Spacing.cardInsets)
            .cardSurface()
            .accessibilityElement(children: .contain)
            if let error = content.lastError {
                Label {
                    Text(error)
                        .font(.callout)
                        .foregroundStyle(Tone.critical.text)
                        .fixedSize(horizontal: false, vertical: true)
                } icon: {
                    Image(systemName: Tone.critical.symbol)
                        .foregroundStyle(Tone.critical.fill)
                        .accessibilityHidden(true)
                }
                .padding(.horizontal, Spacing.s4)
            }
        }
    }

    @ViewBuilder private var icon: some View {
        switch content.icon {
        case .progress:
            DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
                .frame(width: iconSize, height: iconSize)
        case .symbol(let name, let tone):
            Image(systemName: name)
                .stacklySymbol(.bannerSymbol)
                .foregroundStyle(tone.fill)
                .frame(width: iconSize, height: iconSize)
                .accessibilityHidden(true)
        }
    }

    private var iconSize: CGFloat { 24 }

    @ViewBuilder private var trailing: some View {
        if content.isWorking {
            HStack(spacing: Spacing.s8) {
                DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
                    .frame(width: iconSize, height: iconSize)
                actionButton
            }
        } else {
            actionButton
        }
    }

    @ViewBuilder private var actionButton: some View {
        if let action = content.action {
            Button(action.title) { perform(action) }
                .buttonStyle(.bordered)
                .disabled(reason: content.isWorking ? OnboardingCopy.working : nil)
                .focusedIfAvailable(focus, equals: .cardAction)
        }
    }
}

extension View {
    /// Binds focus to `value` when a binding is given.
    @ViewBuilder fileprivate func focusedIfAvailable<V: Hashable>(
        _ binding: FocusState<V?>.Binding?, equals value: V
    ) -> some View {
        if let binding {
            focused(binding, equals: value)
        } else {
            self
        }
    }
}
