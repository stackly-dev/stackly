import SwiftUI

/// The Needs Attention section of the Overview (spec 0022, *Aturan Needs Attention*): the
/// first six items, "Show {n} More" for the rest, a spinner while a source has not answered
/// yet (a source that failed or is unsupported is an item instead), and "Nothing needs
/// attention." when there is nothing.
struct AttentionList: View {
    /// The items, or `nil` while a source has not loaded.
    let items: [AttentionItem]?
    let open: @MainActor (AttentionDestination) -> Void

    @State private var showsAll = false

    var body: some View {
        SectionCard(title: "Needs Attention") {
            if let items {
                if items.isEmpty {
                    InlineEmptyRow(OverviewCopy.nothingNeedsAttention)
                } else {
                    ForEach(shown(items)) { item in
                        AttentionRow(item: item, open: open)
                    }
                    if items.count > OverviewCopy.attentionLimit {
                        toggle(hidden: items.count - OverviewCopy.attentionLimit)
                    }
                }
            } else {
                SectionProgress()
            }
        }
    }

    private func shown(_ items: [AttentionItem]) -> [AttentionItem] {
        showsAll ? items : Array(items.prefix(OverviewCopy.attentionLimit))
    }

    private func toggle(hidden: Int) -> some View {
        Button {
            withAnimation(StacklyMotion.crossfadeAnimation) { showsAll.toggle() }
        } label: {
            Text(showsAll ? OverviewCopy.showLess : OverviewCopy.showMore(hidden))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(Spacing.rowInsets)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(Tone.informative.text)
        .accessibilityIdentifier(AccessibilityID.Overview.attentionToggle)
    }
}

/// One item: the tone symbol, the title, the body, and a chevron, all one button.
struct AttentionRow: View {
    let item: AttentionItem
    let open: @MainActor (AttentionDestination) -> Void

    var body: some View {
        Button {
            open(item.destination)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
                Image(systemName: item.tone.symbol)
                    .foregroundStyle(item.tone.fill)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(item.title)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(.primary)
                    Text(item.message)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(StacklyColor.decorative)
                    .accessibilityHidden(true)
            }
            .padding(Spacing.rowInsets)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(item.spokenLabel)
        .accessibilityHint(item.spokenHint)
        .accessibilityIdentifier(AccessibilityID.Overview.attention(item.id))
    }
}

/// A section of the Overview: a `.headline` title above a card that holds the rows, with a
/// divider between two rows.
struct SectionCard<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            Text(title)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            Group(subviews: content) { rows in
                VStack(spacing: 0) {
                    ForEach(rows) { row in
                        row
                        if row.id != rows.last?.id {
                            Divider().padding(.leading, Spacing.s14)
                        }
                    }
                }
            }
            .cardSurface()
        }
    }
}

/// The spinner of a section whose source has not loaded: it shows after the same delay as the
/// spinner of a page, so a quick load never flashes it.
struct SectionProgress: View {
    @Environment(\.loadingDelay) private var loadingDelay
    var size = CGSize(width: 64, height: 64)

    var body: some View {
        DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
            .frame(maxWidth: .infinity, minHeight: size.height, maxHeight: size.height)
    }
}
