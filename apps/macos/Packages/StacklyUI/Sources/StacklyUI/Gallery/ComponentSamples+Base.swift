#if DEBUG
    import StacklyClient
    import SwiftUI

    /// The sample values of the Components gallery page and of the component previews
    /// (spec 0067, AC-20): one source, taken from the mockup, with no real credentials.
    ///
    /// Each milestone adds its own samples in `ComponentSamples+<part>.swift` as an
    /// extension, so parallel milestones never edit the same file. This file holds the
    /// samples of milestone 1: tiles, key value, copy, actions, and feedback.
    enum ComponentSamples {
        // MARK: - Tiles

        /// The six provider tiles of the mockup, with their provider names.
        static let tiles: [(name: String, spec: TileSpec)] = [
            ("PostgreSQL", TileSpec(mark: "Pg", tile: .blue)),
            ("MySQL", TileSpec(mark: "My", tile: .teal)),
            ("NATS", TileSpec(mark: "Na", tile: .green)),
            ("ClickHouse", TileSpec(mark: "Ch", tile: .yellow)),
            ("Valkey", TileSpec(mark: "Vk", tile: .red)),
            ("Traefik", TileSpec(mark: "Tr", tile: .cyan)),
        ]

        /// The sizes the mockup draws a tile at.
        static let tileSizes: [CGFloat] = [
            ProviderTile.smallSize, ProviderTile.rowSize, ProviderTile.mediumSize,
            ProviderTile.largeSize,
        ]

        /// A tile without a mark: the default mark of the name.
        static let tileWithoutMark = (
            name: "Mosquitto", spec: TileSpec(mark: nil, tile: .orange, name: "Mosquitto")
        )
        /// A tile without a color: gray.
        static let tileWithoutColor = (name: "Unknown", spec: TileSpec(mark: "Uk", tile: nil))
        /// A tile with a color this build does not know: gray.
        static let tileUnrecognized = (
            name: "Newer provider", spec: TileSpec(mark: "Nw", tile: .unrecognized)
        )
    }
#endif
