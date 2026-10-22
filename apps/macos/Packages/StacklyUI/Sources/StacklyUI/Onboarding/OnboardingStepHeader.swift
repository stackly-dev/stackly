import SwiftUI

/// The title and subtitle every step starts with. The title takes the VoiceOver focus when
/// the step changes (spec 0016, AC-10).
struct OnboardingStepHeader: View {
    /// Welcome has the large title; the other steps have `.title.bold()`.
    enum Style {
        case welcome
        case step
    }

    let title: String
    let subtitle: String
    var style: Style = .step
    let focus: AccessibilityFocusState<Bool>.Binding

    var body: some View {
        VStack(spacing: Spacing.s6) {
            Text(title)
                .font(style == .welcome ? Font.largeTitle.bold() : Font.title.bold())
                .foregroundStyle(.primary)
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
                .accessibilityFocused(focus)
            Text(subtitle)
                .font(.body)
                .foregroundStyle(StacklyColor.textSecondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity)
    }
}
