import AppKit
import SwiftUI

/// The onboarding: the whole content of the main window until Get Started or Escape
/// (spec 0016, AC-1 to AC-3 and AC-10).
/// It reads and shows. The only thing it writes is the finished record, through
/// ``OnboardingModel/finish(_:)``, and only from Get Started and Escape.
struct OnboardingFlow: View {
    /// The width of the column the steps sit in.
    static let columnWidth: CGFloat = 520

    let onboarding: OnboardingModel
    /// The icon the Welcome step shows; the app's own by default, a fixed image in tests.
    var appIcon: NSImage = NSApplication.shared.applicationIconImage

    @Environment(\.feedbackCenter) private var feedbackCenter
    @Environment(DaemonModel.self) private var model: DaemonModel?

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                stepContent
                    .frame(maxWidth: Self.columnWidth)
                    .padding(.horizontal, Spacing.pageHorizontal)
                    .padding(.vertical, Spacing.sectionGap)
                    .frame(maxWidth: .infinity)
            }
            Divider()
            footer
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(StacklyColor.surfaceWindow)
        .toolbar(removing: .title)
        .toolbarBackgroundVisibility(.hidden, for: .windowToolbar)
        // Escape skips from any step; an open confirmation takes the key first.

        // The banner this window hides would say `not_found`, so the flow says it, on every
        // step, and only when the window banner would show the agent row.

    }

    @ViewBuilder private var stepContent: some View {
        switch onboarding.step {
        case .welcome:
            WelcomeStep(icon: appIcon)
        case .storage:
            StorageStep()
        case .backgroundAgent:
            BackgroundAgentStep()
        case .catalogTrust:
            CatalogTrustStep()
        case .commandLine:
            CommandLineStep()
        }
    }

    private var footer: some View {
        HStack(spacing: Spacing.s12) {
            OnboardingStepDots(current: onboarding.step) { onboarding.go(to: $0) }
            Text(OnboardingFooter.stepText(onboarding.step))
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
            Spacer(minLength: Spacing.s12)
            if OnboardingFooter.showsBack(step: onboarding.step) {
                Button(OnboardingCopy.back) { onboarding.back() }

            }
            Button(primaryLabel) { primaryAction() }
                .buttonStyle(.borderedProminent)

        }
        .padding(.horizontal, Spacing.pageHorizontal)
        .padding(.vertical, Spacing.s12)
    }

    private var primaryLabel: String {
        OnboardingFooter.primaryLabel(
            step: onboarding.step, agentState: model?.agent?.assessment?.state)
    }

    private func primaryAction() {
        OnboardingActions.primary(onboarding: onboarding, feedback: feedbackCenter)
    }
}
