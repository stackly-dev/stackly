import SwiftUI

/// The sidebar footer: the daemon status and the Settings button.
///
/// The status is a ``StatusLabel`` in the `.row` context, read by VoiceOver as
/// "Status: stacklyd running". The Settings button opens the Settings scene, like ⌘,.
public struct DaemonFooter: View {
    private let status: StatusAppearance

    /// Creates the footer.
    ///
    /// - Parameter status: The footer status from ``ShellPresentation/footer``.
    public init(status: StatusAppearance) {
        self.status = status
    }

    /// The footer's content.
    public var body: some View {
        HStack(spacing: Spacing.s8) {
            StatusLabel(status, context: .row)
                .font(.callout)
                .accessibilityIdentifier(AccessibilityID.DaemonFooter.status)
                .frame(maxWidth: .infinity, alignment: .leading)
            SettingsLink {
                Label("Settings", systemImage: "slider.horizontal.3")
                    .labelStyle(.iconOnly)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .help("Settings (⌘,)")
            .accessibilityLabel("Settings")
        }
        .padding(
            EdgeInsets(
                top: Spacing.s8, leading: Spacing.s16, bottom: Spacing.s8, trailing: Spacing.s8)
        )
    }
}
