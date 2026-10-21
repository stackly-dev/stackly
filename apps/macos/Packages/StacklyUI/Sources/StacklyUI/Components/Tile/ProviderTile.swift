import StacklyClient
import SwiftUI

/// A provider's tile: a rounded square in a system color with a bold white mark in the
/// middle (spec 0067, AC-2).
///
/// The tile is decoration: it is hidden from VoiceOver and the provider name always
/// stands in text next to it. The mark comes from the catalog, or from
/// ``ProviderMark/defaultMark(for:)`` when there is none.
public struct ProviderTile: View {
    /// 18 pt: the Instances table.
    public static let smallSize: CGFloat = 18
    /// 26 pt: the Overview rows and the Installing rows of Runtimes.
    public static let rowSize: CGFloat = 26
    /// 40 pt: the Catalog cards, the sheet header, and the inspector header.
    public static let mediumSize: CGFloat = 40
    /// 52 pt: the Catalog detail panel.
    public static let largeSize: CGFloat = 52

    private let spec: TileSpec
    private let size: CGFloat

    /// Creates a tile from its parts.
    ///
    /// - Parameters:
    ///   - mark: The catalog mark, or `nil` for the default mark of `name`.
    ///   - tile: The catalog color, or `nil` for gray.
    ///   - name: The provider name, for the default mark.
    ///   - size: The side in points, for example ``mediumSize``.
    public init(mark: String?, tile: TileColor?, name: String? = nil, size: CGFloat) {
        self.init(TileSpec(mark: mark, tile: tile, name: name), size: size)
    }

    /// Creates a tile from a ``TileSpec``.
    ///
    /// - Parameters:
    ///   - spec: The mark, color, and name.
    ///   - size: The side in points, for example ``mediumSize``.
    public init(_ spec: TileSpec, size: CGFloat) {
        self.spec = spec
        self.size = size
    }

    /// The mark that is drawn.
    var markText: String {
        if let mark = spec.mark, mark.isEmpty == false {
            return mark
        }
        return ProviderMark.defaultMark(for: spec.name)
    }

    /// The tile's content.
    public var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.27, style: .continuous)
                .fill(Color(nsColor: TileFill.color(for: spec.tile)))
            Text(markText)
                .stacklyTileMark(tileSize: size)
                .foregroundStyle(Color.white)
                .padding(.horizontal, size * 0.08)
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { TileGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { TileGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { TileGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
