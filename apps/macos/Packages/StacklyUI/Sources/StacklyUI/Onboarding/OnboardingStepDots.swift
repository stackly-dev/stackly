import SwiftUI

/// The five dots of the footer, each a button that moves straight to its step (spec 0016,
/// AC-2 and AC-10). The active dot is wider; the width changes without animation, because
/// spec 0012 allows only the crossfade.
struct OnboardingStepDots: View {
    let current: OnboardingStep
    let select: (OnboardingStep) -> Void

    private static let height: CGFloat = 7
    private static let activeWidth: CGFloat = 18

    var body: some View {
        HStack(spacing: 0) {
            ForEach(OnboardingStep.allCases, id: \.self) { step in
                let isCurrent = step == current
                Button {
                    select(step)
                } label: {
                    Capsule()
                        .fill(
                            isCurrent
                                ? AnyShapeStyle(.primary) : AnyShapeStyle(StacklyColor.decorative)
                        )
                        .frame(
                            width: isCurrent ? Self.activeWidth : Self.height, height: Self.height
                        )
                        .frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)
                        .contentShape(Rectangle())
                        .transaction { $0.animation = nil }
                }
                .buttonStyle(.plain)
                .accessibilityLabel(OnboardingFooter.stepSpoken(step))
                .accessibilityAddTraits(isCurrent ? .isSelected : [])
            }
        }
    }
}
