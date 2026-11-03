import Foundation
import StacklyClient

/// What one location row of the Storage tab says and offers (spec 0032 AC-19, AC-24). It is a
/// pure function of the daemon's settings and locations, so the wording is tested without a
/// view.
struct LocationRowContent: Equatable {
    /// An earlier default that something still uses or once used.
    struct Older: Equatable, Identifiable {
        /// The location's identity.
        var id: UUID
        /// The folder.
        var path: String
        /// "Also used by 3 instances" or "No longer used for new downloads".
        var usage: String
        /// "Not reachable" or "Unknown while stacklyd is offline", or `nil` when it is there.
        var status: String?
    }

    /// The kind the row is for.
    var kind: LocationKind
    /// The title of the row.
    var title: String
    /// The default folder, resolved.
    var path: String
    /// "Used for new instances" or "Used for new downloads".
    var usage: String
    /// "Not reachable" or "Unknown while stacklyd is offline", or `nil` when the folder is there.
    var status: String?
    /// Whether Use Default shows: the default is not the one that comes with Stackly.
    var showsUseDefault: Bool
    /// The earlier locations to list under the row.
    var older: [Older]

    /// The title of a location kind.
    static func title(of kind: LocationKind) -> String {
        switch kind {
        case .data: "Instance data"
        case .logs: "Logs"
        case .cache: "Download cache"
        case .unrecognized: "Other"
        }
    }

    /// The sentence that confirms a new default (spec 0032 AC-19).
    static func confirmation(for kind: LocationKind) -> String {
        switch kind {
        case .cache: "New downloads go to this folder. Downloads already saved stay where they are."
        default:
            "Only instances you create from now on use this folder. Existing data stays where it is."
        }
    }

    /// The row for `kind`, or `nil` for a kind this build does not know.
    /// - Parameters:
    ///   - kind: The kind.
    ///   - settings: The settings last read.
    ///   - locations: Every location last read.
    ///   - isConnected: Whether the daemon answers now.
    static func make(
        kind: LocationKind, settings: DaemonSettingsView, locations: [StorageLocationView],
        isConnected: Bool
    ) -> LocationRowContent? {
        let current: StorageLocationView
        switch kind {
        case .data: current = settings.locations.data
        case .logs: current = settings.locations.logs
        case .cache: current = settings.locations.cache
        case .unrecognized: return nil
        }
        let older = locations.filter { $0.kind == kind && $0.id != current.id }
            .compactMap { location -> Older? in
                guard let usage = olderUsage(location, kind: kind) else { return nil }
                return Older(
                    id: location.id, path: location.path, usage: usage,
                    status: status(of: location, isConnected: isConnected))
            }
        return LocationRowContent(
            kind: kind, title: title(of: kind), path: current.path,
            usage: kind == .cache ? "Used for new downloads" : "Used for new instances",
            status: status(of: current, isConnected: isConnected),
            showsUseDefault: current.builtIn == false, older: older)
    }

    private static func status(of location: StorageLocationView, isConnected: Bool) -> String? {
        if isConnected == false { return SettingsCopy.unknownWhileOffline }
        return location.reachable ? nil : "Not reachable"
    }

    /// A location with no instances stays out of the list, though the daemon keeps it; an
    /// old cache folder is always listed, since downloads may still be in it.
    private static func olderUsage(_ location: StorageLocationView, kind: LocationKind) -> String? {
        if kind == .cache { return "No longer used for new downloads" }
        guard location.instanceCount > 0 else { return nil }
        let count = location.instanceCount
        return "Also used by \(count) \(count == 1 ? "instance" : "instances")"
    }

    /// The text of the read only log retention row (spec 0032 AC-21).
    /// - Parameter retention: The numbers the daemon reports.
    static func retentionText(_ retention: LogRetention) -> String {
        let size = "\(retention.segmentMaxBytes / (1024 * 1024)) MiB"
        return
            "Kept per stream: \(retention.segmentsPerStream) files × \(size). Older lines are removed as new ones arrive."
    }
}
