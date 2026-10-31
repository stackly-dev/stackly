import StacklyClient
import SwiftUI

/// The inspector of the Runtimes page for one runtime (spec 0018, *Halaman Runtimes*): the
/// header and the facts `stackly runtime inspect` prints.
/// The facts the list row carries show at once; the rest (path, sizes, signature, the
/// executables, the dependencies, and the compatibility record) wait for `runtime.get`,
/// and a runtime that failed to load shows its failure with Retry. The inspector is a
/// deliberate difference from the mockup, kept for parity with the CLI.
struct RuntimeInspector: View {
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone

    let row: RuntimeRowModel
    let phase: RuntimeDetailModel.Phase
    let retry: @MainActor () -> Void

    private var summary: RuntimeSummary { row.summary }

    private var detail: RuntimeDetail? {
        if case .loaded(let detail) = phase { detail } else { nil }
    }

    var body: some View {
        InspectorHeader(
            title: row.name,
            subtitle: "\(summary.software) · \(RuntimeCopy.arch(summary.target.arch))",
            tile: row.tile, status: row.appearance)
        phaseNote
        CardSection {
            copyable("ID", text: summary.id.uuidString.lowercased(), what: "runtime ID", key: "id")
            LabeledContent("Software", value: summary.software)
            LabeledContent("Version", value: summary.version)
            LabeledContent("Line", value: summary.line)
            LabeledContent("Target") { Text(targetText) }
            LabeledContent("Source", value: row.source)
            LabeledContent("State") { StatusLabel(row.appearance, context: .standard) }
            support
            if let digest = summary.artifactSha256 {
                copyable("Digest", text: "sha256:\(digest)", what: "digest", key: "digest")
            }
            if let artifact = detail?.artifact {
                if let size = artifact.size {
                    LabeledContent(
                        "Download size", value: CatalogCopy.sizeText(size, locale: locale))
                }
                if let size = artifact.unpackedSize {
                    LabeledContent(
                        "Unpacked size", value: CatalogCopy.sizeText(size, locale: locale))
                }
            }
            if let path = detail?.path {
                copyable("Path", text: path, what: "path", key: "path")
            }
            if let signature = detail?.artifact?.signature {
                LabeledContent("Signature") {
                    Text(RuntimeCopy.signatureText(signature)).multilineTextAlignment(.trailing)
                }
            }
            if let installedAt = summary.installedAt {
                LabeledContent("Installed", value: dateText(installedAt))
            }
            if let verifiedAt = summary.lastVerifiedAt {
                LabeledContent("Verified", value: dateText(verifiedAt))
            }
            LabeledContent("Used By") {
                Text(row.usedBy).multilineTextAlignment(.trailing)
            }
            if let executables = detail?.executables, executables.isEmpty == false {
                list("Executables", executables.map { $0 }, monospaced: true)
            }
            if let dependencies = detail?.artifact?.dependencies, dependencies.isEmpty == false {
                list(
                    "Dependencies", dependencies.map(RuntimeCopy.dependencyText), monospaced: false)
            }
            if let warnings = detail?.artifact?.warnings, warnings.isEmpty == false {
                list("Warnings", warnings, monospaced: false)
            }
        }
        .labeledContentStyle(.stackly)

    }

    // MARK: - Pieces

    @ViewBuilder private var phaseNote: some View {
        switch phase {
        case .loading:
            ProgressView()
                .controlSize(.small)
                .frame(maxWidth: .infinity)

        case .failed(let failure):
            StateBanner(
                tone: .critical, title: failure.title, message: failure.message,
                code: failure.code,
                actions: [StateAction(title: "Retry", mutates: false, perform: retry)])
        case .loaded:
            EmptyView()
        }
    }

    /// The Support row: the badge and the reason, then the record that decided it.
    private var support: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            LabeledContent("Support") {
                ToneBadge(CatalogAppearance.appearance(of: summary.support))
            }
            Text(RuntimeCopy.supportText(of: summary))
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            if let record = RuntimeCopy.decidingRecord(of: summary, in: detail?.compatibility) {
                Text(
                    RuntimeCopy.recordText(
                        record, locale: locale, calendar: calendar, timeZone: timeZone)
                )
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func copyable(_ title: String, text: String, what: String, key: String)
        -> some View
    {
        HStack(spacing: Spacing.s8) {
            LabeledContent(title) {
                Text(text)
                    .font(.callout.monospaced())
                    .multilineTextAlignment(.trailing)
                    .fixedSize(horizontal: false, vertical: true)
            }
            CopyButton(text, what: what, style: .icon)
        }
    }

    private func list(_ title: String, _ values: [String], monospaced: Bool) -> some View {
        LabeledContent(title) {
            VStack(alignment: .trailing, spacing: Spacing.s2) {
                ForEach(Array(values.enumerated()), id: \.offset) { _, value in
                    if monospaced {
                        Text(value).font(.callout.monospaced())
                    } else {
                        Text(value)
                    }
                }
            }
            .multilineTextAlignment(.trailing)
        }
    }

    private var targetText: String {
        let os = CatalogCopy.osName(summary.target.os)
        return "\(os) · \(CatalogCopy.archName(summary.target.arch, on: summary.target.os))"
    }

    private func dateText(_ date: Date) -> String {
        RuntimeCopy.dateText(date, locale: locale, calendar: calendar, timeZone: timeZone)
    }
}
