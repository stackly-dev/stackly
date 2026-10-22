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
                Text("Instances").font(.headline).accessibilityAddTraits(.isHeader)
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

/// One instance of the Overview.
struct OverviewInstanceRow: View {
    let row: InstanceRowModel
    let action: StacklyAction
    let open: @MainActor () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            HStack(spacing: Spacing.s12) {
                Button(action: open) {
                    HStack(spacing: Spacing.s12) {
                        ProviderTile(row.tile, size: ProviderTile.rowSize)
                        VStack(alignment: .leading, spacing: Spacing.s2) {
                            Text(row.name)
                                .font(.body.weight(.semibold))
                                .foregroundStyle(.primary)
                            Text(row.meta)
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        StatusLabel(row.appearance, context: .standard)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(row.spokenLabel)
                .accessibilityIdentifier(AccessibilityID.Overview.instance(row.id))
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
}
