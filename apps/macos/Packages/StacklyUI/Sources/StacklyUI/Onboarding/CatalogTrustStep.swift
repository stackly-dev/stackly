import SwiftUI

/// The Catalog Trust step: where the catalog the daemon loaded comes from (spec 0016, AC-7).
///
/// It reads `CatalogStore.info`, `error`, and `unsupported` of the daemon model and calls
/// nothing itself. The toggle is a statement, not a setting: it is always on and disabled.
struct CatalogTrustStep: View {
    let focus: AccessibilityFocusState<Bool>.Binding

    @Environment(DaemonModel.self) private var model: DaemonModel?
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone

    var body: some View {
        let content = CatalogTrustContent.make(
            info: model?.catalog.info, error: model?.catalog.store.error,
            unsupported: model?.catalog.store.unsupported ?? false,
            runtimeInstallAvailable: OnboardingCopy.runtimeInstallAvailable,
            locale: locale, calendar: calendar, timeZone: timeZone)
        VStack(spacing: Spacing.s14) {
            OnboardingStepHeader(
                title: OnboardingCopy.catalogTitle, subtitle: OnboardingCopy.catalogSubtitle,
                focus: focus)
            CatalogTrustRow(content: content)
            Text(OnboardingCopy.catalogNote)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// The row of the Catalog Trust step, from a value ``CatalogTrustContent`` built, so a
/// snapshot or a preview renders any row without a model.
struct CatalogTrustRow: View {
    let content: CatalogTrustContent

    var body: some View {
        CardSection {
            HStack(alignment: .top, spacing: Spacing.s12) {
                VStack(alignment: .leading, spacing: Spacing.s4) {
                    Text(content.label)
                        .font(.body.weight(.semibold))
                    Text(content.detail)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                    if let revision = content.revision {
                        Text(revision)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: Spacing.s12)
                if let toggle = content.toggle {
                    Toggle(toggle, isOn: .constant(true))
                        .labelsHidden()
                        .disabled(true)
                        .help(toggle)
                        .accessibilityLabel(toggle)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
