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

        // MARK: - Key value

        /// One read only row of a card: a label and a value.
        struct KeyValueRow: Identifiable {
            let label: String
            let value: String
            var isCode = false
            var id: String { label }
        }

        /// Summary rows of an instance, from the mockup.
        static let summaryRows = [
            KeyValueRow(label: "Software", value: "PostgreSQL 17.4"),
            KeyValueRow(label: "Endpoint", value: "127.0.0.1:5432", isCode: true),
            KeyValueRow(label: "Created", value: "Oct 12, 2026 at 09:41"),
        ]
        /// A value that does not fit next to its label at 640 pt: it moves below the label.
        static let longValueRow = KeyValueRow(
            label: "Data directory",
            value:
                "/Users/dev/Library/Application Support/Stackly/instances/pg17-dev/data/pg_wal/"
                + "archive_status/000000010000000000000042.ready",
            isCode: true
        )
        /// A digest, which is always code.
        static let digestRow = KeyValueRow(
            label: "Digest",
            value: "sha256:9f2c4b7e1a6d03c85e7f4a2b9c1d8e6f3a5b7c9d1e2f4a6b8c0d2e4f6a8b0c1d",
            isCode: true
        )
        /// The footer of a card with a text footer.
        static let keyValueFooter = "Stackly picks the next free port when you create an instance."
        /// The reason shown in a footer instead of only in a tooltip.
        static let backupReason = "The Valkey provider doesn’t declare a backup capability."
    }
#endif
