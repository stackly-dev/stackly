import SwiftUI

/// The Storage step: where data, cache, logs, and secrets live (spec 0016, AC-6).
///
/// The folders come from the last daemon that answered, or from the macOS defaults before
/// any did. The step reads; it creates nothing and shows no value of `STACKLY_ROOT`.
struct StorageStep: View {
    let focus: AccessibilityFocusState<Bool>.Binding

    @Environment(DaemonModel.self) private var model: DaemonModel?

    var body: some View {
        let content = StorageContent.make(
            roots: model?.lastDaemon?.info?.roots,
            home: UserHome.current().path(percentEncoded: false),
            rootOverride: model?.rootOverrideActive ?? false)
        VStack(spacing: Spacing.s14) {
            OnboardingStepHeader(
                title: OnboardingCopy.storageTitle, subtitle: content.subtitle, focus: focus)
            CardSection {
                ForEach(content.rows, id: \.label) { row in
                    LabeledContent(row.label) {
                        value(of: row)
                            .foregroundStyle(StacklyColor.textSecondary)
                            .multilineTextAlignment(.trailing)
                    }
                }
            }
            .labeledContentStyle(.stackly)
        }
    }

    /// A path in the code style, which also selects; other values in the callout style.
    @ViewBuilder private func value(of row: StorageContent.Row) -> some View {
        if row.isPath {
            Text(row.value).stacklyCode()
        } else {
            Text(row.value).font(.callout)
        }
    }
}
