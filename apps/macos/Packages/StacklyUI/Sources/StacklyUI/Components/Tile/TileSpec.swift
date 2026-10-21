import StacklyClient

/// What a ``ProviderTile`` draws: the mark, the color, and the provider name the default
/// mark comes from.
nonisolated public struct TileSpec: Equatable, Sendable {
    /// The mark: one or two characters from the catalog. `nil` or empty uses
    /// ``ProviderMark/defaultMark(for:)`` of ``name``.
    public var mark: String?
    /// The tile color from the catalog. `nil` and `.unrecognized` draw gray.
    public var tile: TileColor?
    /// The provider name, for the default mark when ``mark`` is `nil`.
    public var name: String?

    /// Creates a tile spec.
    ///
    /// - Parameters:
    ///   - mark: The catalog mark, or `nil` for the default mark.
    ///   - tile: The catalog color, or `nil` for gray.
    ///   - name: The provider name, for the default mark.
    public init(mark: String?, tile: TileColor?, name: String? = nil) {
        self.mark = mark
        self.tile = tile
        self.name = name
    }
}
