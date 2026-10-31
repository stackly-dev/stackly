import StacklyClient
import SwiftUI

/// The Instances section of the Overview (spec 0022, *Seksi Instances*): the first six
/// instances by name, each a button that opens it, with its main action next to it.
struct OverviewInstanceList: View {
    let rows: [InstanceRowModel]
    let total: Int
    let action: (InstanceSummary) -> StacklyAction
    let open: @MainActor (UUID) -> Void
    let showAll: @MainActor () -> Void
    let browseCatalog: @MainActor () -> Void

    var body: some View {
        if rows.isEmpty {
            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text("Instances").font(.headline)
                StatePlaceholder(
                    InstancesPage.noInstances(browseCatalog: browseCatalog, newInstance: nil),
                    size: .section
                )
                .frame(maxWidth: .infinity)
                .cardSurface()
            }
        } else {
            SectionCard(title: "Instances") {
                ForEach(rows) { row in
                    OverviewInstanceRow(
                        row: row, action: action(row.summary), open: { open(row.id) })
                }
                if total > rows.count {
                    Button(action: showAll) {
                        Text(OverviewCopy.showAllInstances(total))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(Spacing.rowInsets)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(Tone.informative.text)
                }
            }
        }
    }
}

/// One instance of the Overview. The status stands next to the name when the whole status
/// fits there, else under the provider line, so it is never cut at the 640 pt content width
/// (spec 0022 AC-13).
struct OverviewInstanceRow: View {
    let row: InstanceRowModel
    let action: StacklyAction
    let open: @MainActor () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            HStack(spacing: Spacing.s12) {
                Button(action: open) {
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: Spacing.s12) {
                            tile
                            nameAndMeta
                                .frame(maxWidth: .infinity, alignment: .leading)
                            // An HStack offers a label half of the free width, and the
                            // detail would be cut although the row has room for it.
                            status
                                .fixedSize(horizontal: true, vertical: false)
                        }
                        HStack(spacing: Spacing.s12) {
                            tile
                            VStack(alignment: .leading, spacing: Spacing.s4) {
                                nameAndMeta
                                status
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)

                ActionButton(action, showsReason: false)
                    .controlSize(.small)
                    .help(action.disabledReason ?? action.title)
            }
            if let reason = action.disabledReason {
                DisabledReasonText(reason)
            }
        }
        .padding(Spacing.rowInsets)
    }

    private var tile: some View {
        ProviderTile(row.tile, size: ProviderTile.rowSize)
    }

    private var nameAndMeta: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            Text(row.name)
                .font(.body.weight(.semibold))
                .foregroundStyle(.primary)
            Text(row.meta)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var status: some View {
        StatusLabel(row.appearance, context: .standard)
    }
}
