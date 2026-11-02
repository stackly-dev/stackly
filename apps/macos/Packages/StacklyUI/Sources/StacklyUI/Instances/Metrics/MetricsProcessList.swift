import SwiftUI

/// The Processes list of the Metrics tab (spec 0028, AC-13): the whole tree of the newest
/// sample, whatever scope the cards show. A name is indented by its depth; a value that could
/// not be measured reads "—", with the reason as its help text.
struct MetricsProcessList: View {
    let lines: [MetricsFormat.ProcessLine]
    let truncated: Bool
    /// The processes of the tree in the newest sample, for the count of the rows not shown.
    let treeCount: UInt64?

    private static let pidWidth: CGFloat = 70
    private static let memoryWidth: CGFloat = 96
    private static let cpuWidth: CGFloat = 70
    private static let indent: CGFloat = 12

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            Text("Processes").font(.headline)
            VStack(spacing: 0) {
                row(
                    pid: Text("PID"), name: Text("Name"), memory: Text("Memory"), cpu: Text("CPU")
                )
                .font(.footnote)
                .foregroundStyle(StacklyColor.textSecondary)
                if lines.isEmpty {
                    Divider()
                    InlineEmptyRow("Waiting for the first sample.")
                }
                ForEach(lines) { line in
                    Divider()
                    row(
                        pid: Text(String(line.id)).foregroundStyle(StacklyColor.textSecondary),
                        name: Text(line.name)
                            .font(.callout.monospaced())
                            .padding(.leading, CGFloat(line.depth) * Self.indent),
                        memory: reading(line.memory), cpu: reading(line.cpu))
                }
                if truncated {
                    Divider()
                    Text(MetricsFormat.moreProcesses(shown: lines.count, total: treeCount))
                        .font(.footnote)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .padding(Spacing.compactRowInsets)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .cardSurface()
        }
    }

    private func row(
        pid: some View, name: some View, memory: some View, cpu: some View
    ) -> some View {
        HStack(spacing: Spacing.s12) {
            pid.frame(width: Self.pidWidth, alignment: .leading)
            name.lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            memory.frame(width: Self.memoryWidth, alignment: .trailing)
            cpu.frame(width: Self.cpuWidth, alignment: .trailing)
        }
        .stacklyNumeric()
        .padding(Spacing.compactRowInsets)
    }

    @ViewBuilder private func reading(_ reading: MetricsFormat.Reading) -> some View {
        switch reading {
        case .value(let text):
            Text(text)
        case .unavailable:
            Text("—")
                .foregroundStyle(StacklyColor.textSecondary)
                .help(reading.note ?? "Not available")
        }
    }
}
