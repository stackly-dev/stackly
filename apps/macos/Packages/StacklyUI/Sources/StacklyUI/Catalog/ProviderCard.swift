import StacklyClient
import SwiftUI

/// The card of one provider in the Catalog grid (spec 0017, *Layar Catalog (GUI)*): the
/// tile, the name, the status badge, the meta line, and two lines of description.
///
/// The card is one button, so Tab reaches it and Return or Space chooses it. The chosen
/// card has a ring. A planned provider reads in secondary text on a neutral tile, with no
/// opacity that would lower the contrast. VoiceOver reads "{name}, {category}, {status}";
/// the tile is decoration.
struct ProviderCard: View {
    let provider: ProviderSummary
    let isSelected: Bool
    let select: () -> Void

    private var isPlanned: Bool { provider.state == .planned }
    private var status: StatusAppearance { CatalogAppearance.appearance(of: provider.state) }

    var body: some View {
        Button(action: select) {
            HStack(alignment: .top, spacing: Spacing.s12) {
                ProviderTile(
                    TileSpec(
                        mark: provider.mark, tile: isPlanned ? .gray : provider.tile,
                        name: provider.displayName),
                    size: ProviderTile.mediumSize)
                VStack(alignment: .leading, spacing: Spacing.s4) {
                    titleRow
                    Text(CatalogCopy.metaLine(of: provider))
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                    if let description = provider.description {
                        Text(description)
                            .font(.callout)
                            .foregroundStyle(
                                isPlanned
                                    ? AnyShapeStyle(StacklyColor.textSecondary)
                                    : AnyShapeStyle(.primary)
                            )
                            .lineLimit(2)
                            .padding(.top, Spacing.s2)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(Spacing.cardInsets)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .cardSurface()
            .overlay {
                if isSelected {
                    RoundedRectangle(cornerRadius: Radius.medium.value, style: .continuous)
                        .strokeBorder(Color(nsColor: .controlAccentColor), lineWidth: 2)
                }
            }
            .contentShape(RoundedRectangle(cornerRadius: Radius.medium.value))
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Self.spokenLabel(for: provider))
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .accessibilityIdentifier(AccessibilityID.Catalog.card(provider.provider))
    }

    /// What VoiceOver reads: "{name}, {category}, {status}".
    static func spokenLabel(for provider: ProviderSummary) -> String {
        let category = CatalogCopy.categoryName(provider.category)
        let status = CatalogAppearance.appearance(of: provider.state).label
        return "\(provider.displayName), \(category), \(status)"
    }

    /// The name, which wraps, and the badge on its right.
    private var titleRow: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            name
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            ToneBadge(status)
        }
    }

    private var name: some View {
        Text(provider.displayName)
            .font(.body.weight(.semibold))
            .foregroundStyle(
                isPlanned ? AnyShapeStyle(StacklyColor.textSecondary) : AnyShapeStyle(.primary))
    }
}
