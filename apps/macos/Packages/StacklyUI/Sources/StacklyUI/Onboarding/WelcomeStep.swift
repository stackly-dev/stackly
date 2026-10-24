import AppKit
import SwiftUI

/// The Welcome step: the app icon, the title, and what Stackly is, without promising
/// anything this build does not have (spec 0016, AC-4).
struct WelcomeStep: View {
    /// The app icon; a test or a snapshot passes a fixed image.
    let icon: NSImage

    private static let iconSize: CGFloat = 112
    private static let tileSize: CGFloat = 34

    var body: some View {
        VStack(spacing: Spacing.s14) {
            Image(nsImage: icon)
                .resizable()
                .frame(width: Self.iconSize, height: Self.iconSize)

            OnboardingStepHeader(
                title: OnboardingCopy.welcomeTitle, subtitle: OnboardingCopy.welcomeSubtitle,
                style: .welcome)
            VStack(alignment: .leading, spacing: Spacing.s16) {
                row(OnboardingCopy.isolatedInstances) {
                    SectionIcon(.instances)
                }
                if OnboardingCopy.runtimeInstallAvailable {
                    row(OnboardingCopy.verifiedRuntimes) {
                        SectionIcon(.runtimes)
                    }
                }
                row(OnboardingCopy.noAccount) {
                    Image(systemName: "laptopcomputer")
                        .foregroundStyle(StacklyColor.textSecondary)

                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.top, Spacing.s8)
        }
    }

    /// A feature row: a neutral tile with its symbol, the title, and the detail.
    private func row<Icon: View>(
        _ feature: OnboardingCopy.Feature, @ViewBuilder icon: () -> Icon
    ) -> some View {
        HStack(alignment: .top, spacing: Spacing.s12) {
            icon()
                .frame(width: Self.tileSize, height: Self.tileSize)
                .toneSurface(
                    .neutral, on: .window, strength: .badge,
                    in: RoundedRectangle(cornerRadius: Radius.small.value))
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(feature.title)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.primary)
                Text(feature.detail)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }

    }
}
