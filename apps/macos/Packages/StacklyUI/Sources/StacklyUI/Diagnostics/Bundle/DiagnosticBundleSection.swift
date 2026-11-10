import SwiftUI

/// The Diagnostic Bundle section of Diagnostics (spec 0034, *GUI*): what a bundle holds and
/// Preview Bundle…, which opens the sheet that lists every entry before anything is saved.
struct DiagnosticBundleSection: View {
    let isConnected: Bool
    let preview: @MainActor () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            Text(DoctorCopy.bundleSectionTitle)
                .font(.headline)
            VStack(alignment: .leading, spacing: Spacing.s12) {
                Text(DoctorCopy.bundleSectionText)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                Button(DoctorCopy.previewBundle, action: preview)
                    .buttonStyle(.bordered)
                    .disabled(!isConnected)
            }
            .padding(Spacing.s14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .cardSurface()
        }
    }
}
