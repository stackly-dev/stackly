import SwiftUI

/// The banner over a tab while stacklyd cannot be reached (spec 0032 AC-24).
struct SettingsOfflineBanner: View {
    let hasLoaded: Bool

    var body: some View {
        StateBanner(
            tone: .caution, title: SettingsCopy.offlineTitle,
            message: hasLoaded ? SettingsCopy.offlineMessage : nil)
    }
}

/// A row of a daemon setting: the title and note on the left, the control on the right, a
/// small progress indicator while the change is on its way, "last known" while stacklyd is
/// offline, and the reason a change did not go through under it.
struct DaemonSettingRow<Control: View>: View {
    let title: String
    var note: String?
    let mode: SettingsRowMode
    var isSaving = false
    var notice: String?
    @ViewBuilder let control: Control

    var body: some View {
        LabeledContent {
            if mode == .unavailable {
                Text(SettingsCopy.notAvailable)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            } else {
                HStack(spacing: Spacing.s8) {
                    if isSaving {
                        ProgressView().controlSize(.small)
                    }
                    if mode == .lastKnown {
                        Text(SettingsCopy.lastKnown)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                    control
                        .labelsHidden()
                        .disabled(mode != .live)
                }
            }
        } label: {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(title)
                if let note {
                    Text(note)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let notice {
                    Text(notice)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }
}
