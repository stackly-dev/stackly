import StacklyClient
import SwiftUI

/// The support of each version on each OS (spec 0017, *Layar Catalog (GUI)*): a native
/// `Grid` with the columns Version, macOS, Linux, and Windows, each cell a ``ToneBadge``
/// whose help is the reason. Where the panel is too narrow for four columns, each version
/// becomes a short list of its three OSes.
///
/// Every status is written, and the reason of every cell is also written in
/// ``notes(of:)`` under the matrix, so nothing depends on color or on hover. VoiceOver
/// reads each cell as "{OS}, {status}: {reason}".
struct VersionMatrix: View {
    /// The OS columns, in the order the daemon sends them.
    static let systems: [TargetOs] = [.macos, .linux, .windows]

    let versions: [CatalogVersion]
    /// What the Get column needs, or `nil` for a matrix without actions.
    var runtimeGet: RuntimeGetSource?

    var body: some View {
        ViewThatFits(in: .horizontal) {
            grid
            stacked
        }
        .accessibilityIdentifier(AccessibilityID.Catalog.matrix)
    }

    private var grid: some View {
        Grid(alignment: .leading, horizontalSpacing: Spacing.s12, verticalSpacing: Spacing.s8) {
            GridRow {
                header("Version")
                ForEach(Self.systems, id: \.self) { os in
                    header(CatalogCopy.osName(os))
                }
                if runtimeGet != nil {
                    header("Runtime")
                }
            }
            ForEach(versions, id: \.version) { version in
                GridRow {
                    Text(version.version)
                        .font(.body.weight(.semibold))
                    ForEach(Self.systems, id: \.self) { os in
                        cell(of: version, on: os)
                    }
                    if let runtimeGet {
                        RuntimeGetAction(source: runtimeGet, version: version)
                    }
                }
            }
        }
        .fixedSize(horizontal: true, vertical: false)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var stacked: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            ForEach(versions, id: \.version) { version in
                VStack(alignment: .leading, spacing: Spacing.s6) {
                    Text(version.version)
                        .font(.body.weight(.semibold))
                    ForEach(Self.systems, id: \.self) { os in
                        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                            Text(CatalogCopy.osName(os))
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                                .frame(width: 64, alignment: .leading)
                            cell(of: version, on: os)
                        }
                    }
                    if let runtimeGet,
                        RuntimeGetCell.hostArtifact(of: version, host: runtimeGet.host) != nil
                    {
                        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                            Text("Runtime")
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                                .frame(width: 64, alignment: .leading)
                            RuntimeGetAction(source: runtimeGet, version: version)
                        }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func header(_ title: String) -> some View {
        Text(title)
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(StacklyColor.textSecondary)
    }

    @ViewBuilder
    private func cell(of version: CatalogVersion, on os: TargetOs) -> some View {
        if let support = version.support?.first(where: { $0.os == os }) {
            ToneBadge(CatalogAppearance.appearance(of: support.status))
                .help(CatalogCopy.reasonText(for: support) ?? "")
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(CatalogCopy.spokenText(for: support))
        } else {
            Text("Not listed")
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
        }
    }

    /// The lines under the matrix: one per unique OS, status, and reason, each as
    /// "{OS}: {reason}". Cells without a reason add none.
    static func notes(of versions: [CatalogVersion]) -> [String] {
        struct Key: Hashable {
            let os: TargetOs
            let status: SupportStatus
            let reason: SupportReason
            let note: String?
            let arches: [TargetArch]
        }
        var seen: Set<Key> = []
        var lines: [String] = []
        for version in versions {
            for cell in version.support ?? [] {
                guard let reason = cell.reason, let text = CatalogCopy.reasonText(for: cell)
                else { continue }
                let key = Key(
                    os: cell.os, status: cell.status, reason: reason, note: cell.note,
                    arches: cell.arches ?? [])
                if seen.insert(key).inserted {
                    lines.append("\(CatalogCopy.osName(cell.os)): \(text)")
                }
            }
        }
        return lines
    }
}
