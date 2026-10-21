#if DEBUG
    import StacklyClient
    import SwiftUI

    /// The samples of milestone 3: the inspector (spec 0067, AC-20). Values come from the
    /// mockup, with no real credentials.
    extension ComponentSamples {
        // MARK: - Inspector

        /// The tile of the selected instance.
        static let inspectorTile = TileSpec(mark: "Pg", tile: .blue, name: "PostgreSQL")
        /// The title of the selected instance.
        static let inspectorTitle = "pg17-dev"
        /// The line under the title.
        static let inspectorSubtitle = "PostgreSQL 17.4"
        /// A running, ready, healthy instance.
        static let inspectorStatus = StatusAppearance.instance(
            lifecycle: .running, readiness: .ready, health: .healthy)
        /// A title that does not fit one line at the narrowest column.
        static let inspectorLongTitle = "pg17-analytics-replica-eu-central-staging"
        /// A subtitle that does not fit one line at the narrowest column.
        static let inspectorLongSubtitle = "PostgreSQL 17.4 with the replication extension"
        /// A stopped instance.
        static let inspectorStoppedStatus = StatusAppearance.instance(
            lifecycle: .stopped, readiness: .unknown, health: .unknown)
        /// The message of the placeholder.
        static let inspectorPlaceholderMessage = "Select an instance to see its details."
        /// The width the inspector column opens at, for the column samples.
        static let inspectorColumnWidth = InspectorMetrics.idealWidth
        /// The width of the narrowest inspector column.
        static let inspectorNarrowColumnWidth = InspectorMetrics.minWidth
    }
#endif
