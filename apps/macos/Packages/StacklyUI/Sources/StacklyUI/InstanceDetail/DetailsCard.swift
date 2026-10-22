import StacklyClient
import SwiftUI

/// The Details card of the Summary tab (spec 0021, *Tab Summary*): the facts of the
/// instance, each in the words of ``DetailFormat``. A row that needs the detail shows a
/// delayed spinner until it arrives, while the rows of the summary are there at once.
struct DetailsCard: View {
    @Environment(\.fixtureNow) private var fixtureNow

    let summary: InstanceSummary
    let model: InstanceDetailModel
    let home: String

    var body: some View {
        let detail = model.detail
        CardSection("Details") {
            idRow
            LabeledContent("Runtime") {
                Text(summary.runtimeId.uuidString.lowercased()).font(.callout.monospaced())
            }
            LabeledContent("Version") {
                Text(
                    DetailFormat.version(
                        software: summary.software, detail: detail,
                        runtimeVersion: summary.runtimeVersion))
            }
            digestRow(detail)
            LabeledContent("Provider") {
                Text(
                    detail.map { DetailFormat.provider($0.provider) }
                        ?? "\(summary.provider.name) \(summary.provider.revision)")
            }
            dataRow(detail)
            dataSizeRow
            LabeledContent("Uptime") { uptime }
            if let restarts = DetailFormat.restartAttempts(summary.restart) {
                LabeledContent("Restart attempts") { Text(restarts) }
            }
            LabeledContent("Autostart") {
                if let detail {
                    Text(DetailFormat.autostart(detail.startup))
                } else {
                    placeholder
                }
            }
        }
        .labeledContentStyle(.stackly)
        .accessibilityIdentifier(AccessibilityID.InstanceDetail.card("details"))
    }

    // MARK: - Rows

    private var idRow: some View {
        let id = summary.id.uuidString.lowercased()
        return HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            LabeledContent("Instance ID") {
                Text(id).font(.callout.monospaced())
            }
            CopyButton(
                id, what: "instance ID", style: .icon, announcement: "ID copied.",
                key: "instanceId")
        }
    }

    @ViewBuilder
    private func digestRow(_ detail: InstanceDetail?) -> some View {
        LabeledContent("Digest") {
            if let runtime = detail?.runtime {
                Text(DetailFormat.digest(runtime))
                    .font(.callout.monospaced())
                    .help(DetailFormat.fullDigest(runtime) ?? "External runtime")
                    .accessibilityLabel(
                        DetailFormat.fullDigest(runtime).map { "Digest, \($0)" }
                            ?? "Digest, external runtime")
            } else {
                placeholder
            }
        }
    }

    @ViewBuilder
    private func dataRow(_ detail: InstanceDetail?) -> some View {
        LabeledContent("Data") {
            if let detail {
                Text(DetailFormat.dataPath(detail.dataPath, home: home))
                    .font(.callout.monospaced())
                    .help(detail.dataPath)
                    .accessibilityValue(detail.dataPath)
            } else {
                placeholder
            }
        }
    }

    private var dataSizeRow: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            LabeledContent("Data size") {
                VStack(alignment: .trailing, spacing: Spacing.s2) {
                    Text(DetailFormat.dataSize(model.dataSizeState))
                    if let usage = model.usage, model.dataSizeState != .loading {
                        Text(DetailFormat.scanned(usage, now: fixtureNow ?? Date()))
                            .font(.footnote)
                    }
                }
            }
            Button {
                Task { await model.loadUsage(refresh: true) }
            } label: {
                Image(systemName: "arrow.clockwise")
                    .foregroundStyle(StacklyColor.textSecondary)
                    .frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .disabled(model.isLoadingUsage)
            .accessibilityLabel("Recalculate data size")
            .help("Recalculate data size")
            .accessibilityIdentifier(AccessibilityID.InstanceDetail.recalculate)
        }
    }

    @ViewBuilder private var uptime: some View {
        if summary.process == nil {
            Text("—")
        } else if let fixtureNow {
            Text(DetailFormat.uptime(summary, now: fixtureNow))
        } else {
            TimelineView(.periodic(from: .now, by: 1)) { context in
                Text(DetailFormat.uptime(summary, now: context.date))
            }
        }
    }

    private var placeholder: some View {
        Group {
            if model.isDetailPending {
                DetailPending()
            } else {
                Text("—")
            }
        }
    }
}
