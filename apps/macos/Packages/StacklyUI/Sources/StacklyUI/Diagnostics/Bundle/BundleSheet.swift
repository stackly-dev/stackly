import StacklyClient
import SwiftUI

/// The Export Diagnostic Bundle sheet (spec 0034, AC-19): the daemon builds a preview, the
/// sheet lists every entry (path, description, size) with the redaction note, and Export…
/// opens the save panel in Downloads. On success the sheet closes and the page shows "Saved
/// {file} to {folder}." with Show in Finder; an error stays in the sheet.
struct BundleSheet: View {
    let model: BundleModel
    let chooser: any BundleDestinationChoosing
    let exported: @MainActor (ExportedBundle) -> Void

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        StacklySheet(title: DoctorCopy.bundleSheetTitle, primary: primary, minWidth: 560) {
            BundleSheetContent(model: model)
        }
        .task {
            if model.preview == nil { await model.load() }
        }
    }

    /// Export… once the preview is there; Try Again after the preview failed.
    private var primary: StacklyAction {
        if model.preview == nil, model.failure != nil, model.loading == false {
            return StacklyAction(
                id: "bundle.reload", title: DoctorCopy.tryAgain, mutates: false,
                perform: { Task { await model.load() } })
        }
        return StacklyAction(
            id: "bundle.export", title: DoctorCopy.export, mutates: false,
            disabledReason: model.preview == nil ? DoctorCopy.preparing : nil,
            isBusy: model.exporting || model.loading,
            perform: {
                Task {
                    guard let preview = model.preview,
                        let destination = await chooser.choose(
                            suggestedName: preview.suggestedFileName)
                    else { return }
                    if let result = await model.export(to: destination) {
                        dismiss()
                        exported(result)
                    }
                }
            })
    }
}

/// The body of the bundle sheet, apart from the sheet so a snapshot can render it.
struct BundleSheetContent: View {
    let model: BundleModel

    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            if let notice = model.notice {
                StateBanner(tone: .caution, title: notice)
            }
            if let failure = model.failure {
                StateBanner(
                    tone: .critical,
                    title: model.preview == nil
                        ? DoctorCopy.bundleFailedTitle : DoctorCopy.exportFailedTitle,
                    message: DaemonText.sentence(failure.message), code: failure.code)
            }
            if let preview = model.preview {
                CardSection {
                    ForEach(preview.entries ?? [], id: \.path) { entry in
                        BundleEntryRow(
                            entry: entry, size: BundleSizeText.entry(entry, locale: locale))
                    }
                }
                Text(DoctorCopy.bundleRedaction)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                Text(BundleSizeText.summary(preview, locale: locale))
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            } else if model.failure == nil {
                VStack(spacing: Spacing.s8) {
                    DelayedProgress()
                    Text(DoctorCopy.preparing)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
                .frame(maxWidth: .infinity, minHeight: 160)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// One entry of the preview: the path in monospace, its description, and its size.
private struct BundleEntryRow: View {
    let entry: BundleEntry
    let size: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(entry.path)
                    .font(.callout.monospaced())
                Text(entry.description)
                    .font(.subheadline)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: Spacing.s8)
            Text(size)
                .font(.subheadline)
                .foregroundStyle(StacklyColor.textSecondary)
                .monospacedDigit()
        }
    }
}

/// The sizes of the bundle as the sheet writes them, in the person's locale.
nonisolated enum BundleSizeText {
    /// `412 kB`, `2,000 lines · 412 kB`, with `· truncated` when the entry was cut.
    static func entry(_ entry: BundleEntry, locale: Locale = .current) -> String {
        var parts: [String] = []
        if let lines = entry.lines {
            parts.append("\(lines.formatted(.number.locale(locale))) lines")
        }
        parts.append(CatalogCopy.sizeText(entry.bytes, locale: locale))
        if entry.truncated {
            parts.append("truncated")
        }
        return parts.joined(separator: " · ")
    }

    /// `{n} files · {size} · {zip} as a zip`.
    static func summary(_ preview: BundlePreview, locale: Locale = .current) -> String {
        let count = preview.entries?.count ?? 0
        let files = count == 1 ? "1 file" : "\(count) files"
        let total = CatalogCopy.sizeText(preview.totalBytes, locale: locale)
        let zip = CatalogCopy.sizeText(preview.zipBytes, locale: locale)
        return "\(files) · \(total) · \(zip) as a zip"
    }
}
