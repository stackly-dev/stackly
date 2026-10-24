import SwiftUI

/// The header of an inspector (spec 0067, AC-18): a 40 pt tile, the title, a subtitle, the
/// status, and an optional row of actions, all in the middle of the column.
/// The title is `.title2.weight(.semibold)`. The texts are centered and wrap, so nothing is
/// cut at the narrowest column. The tile is decoration, so the title always stands in text.
/// The actions are usually ``ActionButton``s; they sit side by side when they fit and
/// stack when they do not.
public struct InspectorHeader<Actions: View>: View {
    private let title: String
    private let subtitle: String?
    private let tile: TileSpec?
    private let status: StatusAppearance?
    private let actions: Actions

    /// Creates a header with actions.
    /// - Parameters:
    ///   - title: The name of the selected item.
    ///   - subtitle: A second line, for example the provider and version.
    ///   - tile: The provider tile, or `nil` for none.
    ///   - status: The status, shown as a ``StatusLabel`` in the standard context.
    ///   - actions: The actions of the item, usually ``ActionButton``s.
    public init(
        title: String,
        subtitle: String? = nil,
        tile: TileSpec? = nil,
        status: StatusAppearance? = nil,
        @ViewBuilder actions: () -> Actions
    ) {
        self.title = title
        self.subtitle = subtitle
        self.tile = tile
        self.status = status
        self.actions = actions()
    }

    /// The header's content.
    public var body: some View {
        VStack(spacing: Spacing.s6) {
            if let tile {
                ProviderTile(tile, size: ProviderTile.mediumSize)
                    .padding(.bottom, Spacing.s2)
            }
            Text(title)
                .font(.title2.weight(.semibold))
                .foregroundStyle(.primary)

            if let subtitle {
                Text(subtitle)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            if let status {
                StatusLabel(status, context: .standard)
                    .padding(.top, Spacing.s2)
            }
            if Actions.self != EmptyView.self {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: Spacing.s8) { actions }
                    VStack(spacing: Spacing.s8) { actions }
                }
                .padding(.top, Spacing.s6)
            }
        }
        .multilineTextAlignment(.center)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity)
    }
}

extension InspectorHeader where Actions == EmptyView {
    /// Creates a header without actions.
    /// - Parameters:
    ///   - title: The name of the selected item.
    ///   - subtitle: A second line, for example the provider and version.
    ///   - tile: The provider tile, or `nil` for none.
    ///   - status: The status, shown as a ``StatusLabel`` in the standard context.
    public init(
        title: String,
        subtitle: String? = nil,
        tile: TileSpec? = nil,
        status: StatusAppearance? = nil
    ) {
        self.init(title: title, subtitle: subtitle, tile: tile, status: status) {
            EmptyView()
        }
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { InspectorHeaderGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { InspectorHeaderGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { InspectorHeaderGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
