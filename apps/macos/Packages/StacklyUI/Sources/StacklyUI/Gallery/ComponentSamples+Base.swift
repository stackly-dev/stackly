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

        // MARK: - Copy and console

        /// A command the CLI prints for an instance (mockup).
        static let command =
            "stackly instance create --software postgresql --version 17.4 --name pg17-dev"
        /// A command that does not fit in one line at 640 pt and wraps.
        static let longCommand =
            "stackly instance create --software postgresql --version 17.4 --name pg17-dev "
            + "--port 5433 --data-dir ~/Library/Application\\ Support/Stackly/instances/pg17-dev/data "
            + "--set shared_buffers=256MB --set max_connections=200"
        /// Several lines, such as the example of the onboarding page.
        static let multilineText = """
            # Put Stackly's tools on your PATH
            export PATH="$HOME/Library/Application Support/Stackly/bin:$PATH"
            stackly daemon status
            """

        // MARK: - Actions

        /// The confirmation of removing an instance (mockup copy).
        static let removeConfirmation = ActionConfirmation(
            title: "Remove “pg17-dev”?",
            message: "The instance is removed from Stackly. Its data directory is kept.",
            actionTitle: "Remove"
        )
        /// The main action of a page.
        static let startAction = StacklyAction(
            id: "gallery.start", title: "Start", symbol: "play.fill", mutates: true, perform: {})
        /// An ordinary secondary action.
        static let stopAction = StacklyAction(
            id: "gallery.stop", title: "Stop", symbol: "stop.fill", mutates: true, perform: {})
        /// A destructive action: red, bordered, and confirmed.
        static let removeAction = StacklyAction(
            id: "gallery.remove", title: "Remove…", symbol: "trash",
            role: .destructive(removeConfirmation), mutates: true, perform: {})
        /// An action that is running.
        static let busyAction = StacklyAction(
            id: "gallery.restart", title: "Restart", symbol: "arrow.clockwise", mutates: true,
            isBusy: true, perform: {})
        /// An action the provider does not support, disabled with its reason shown.
        static let unsupportedAction = StacklyAction(
            id: "gallery.backup", title: "Back Up…", symbol: "externaldrive", mutates: true,
            disabledReason: backupReason, perform: {})
        /// A read only action, which the gate never disables.
        static let copyAction = StacklyAction(
            id: "gallery.copy", title: "Copy Connection String", symbol: "doc.on.doc",
            mutates: false, perform: {})
        /// The reason the mutation gate gives while the daemon is unreachable.
        static let gateReason =
            "stacklyd is unavailable. Actions are disabled until the daemon is reachable."
        /// The sections of the More menu sample: ordinary, unsupported, then destructive.
        static let menuSections = [
            [copyAction, startAction],
            [busyAction, unsupportedAction],
            [removeAction],
        ]
    }
#endif
