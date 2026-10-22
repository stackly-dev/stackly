import SwiftUI

/// The onboarding: the whole content of the main window until Get Started or Escape
/// (spec 0016, AC-1 to AC-3 and AC-10).
///
/// It reads and shows. The only thing it writes is the finished record, through
/// ``OnboardingModel/finish(_:)``, and only from Get Started and Escape.
struct OnboardingFlow: View {
    /// The width of the column the steps sit in.
    static let columnWidth: CGFloat = 520

    let onboarding: OnboardingModel

    @Environment(\.feedbackCenter) private var feedbackCenter
    @AccessibilityFocusState private var titleFocused: Bool

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
        .onExitCommand { onboarding.finish(.skipped) }
        .accessibilityAction(.escape) { onboarding.finish(.skipped) }
        .onChange(of: onboarding.step) { _, step in
            titleFocused = true
            announce(OnboardingFooter.announcement(step))
        }
    }

    @ViewBuilder private var stepContent: some View {
        switch onboarding.step {
        // provisional: F16 steps 3 and later milestones replace these with their steps.
        case .welcome:
            OnboardingStepHeader(
                title: OnboardingCopy.welcomeTitle, subtitle: OnboardingCopy.welcomeSubtitle,
                style: .welcome, focus: $titleFocused)
        case .storage:
            OnboardingStepHeader(
                title: OnboardingCopy.storageTitle, subtitle: OnboardingCopy.storageSubtitle,
                focus: $titleFocused)
        case .backgroundAgent:
            OnboardingStepHeader(
                title: OnboardingCopy.agentTitle, subtitle: OnboardingCopy.agentSubtitle,
                focus: $titleFocused)
        case .catalogTrust:
            OnboardingStepHeader(
                title: OnboardingCopy.catalogTitle, subtitle: OnboardingCopy.catalogSubtitle,
                focus: $titleFocused)
        case .commandLine:
            OnboardingStepHeader(
                title: OnboardingCopy.commandLineTitle,
                subtitle: OnboardingCopy.commandLineSubtitle, focus: $titleFocused)
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
                .keyboardShortcut(.defaultAction)
        }
        .padding(.horizontal, Spacing.pageHorizontal)
        .padding(.vertical, Spacing.s12)
    }

    // provisional: F16 milestone 2 passes the agent state of the daemon model here.
    private var primaryLabel: String {
        OnboardingFooter.primaryLabel(step: onboarding.step, agentState: nil)
    }

    /// Continue and Not Now only move on; Get Started finishes and shows the good news.
    private func primaryAction() {
        if onboarding.step == .commandLine {
            onboarding.finish(.getStarted)
            feedbackCenter?.show(.success(OnboardingCopy.setupComplete))
        } else {
            onboarding.next()
        }
    }
}
