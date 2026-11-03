import Foundation
import StacklyClient

/// How a row owned by the daemon behaves now (spec 0032 AC-24).
enum SettingsRowMode: Equatable, Sendable {
    /// The daemon answers, so the row shows its value and can change it.
    case live
    /// The daemon cannot be reached; the row shows the value it last knew, disabled.
    case lastKnown
    /// The daemon cannot be reached and never answered in this session.
    case unavailable
}

/// The words of the Settings window that more than one tab uses, and the rule that picks a
/// row's mode (spec 0032, *Baris Settings*, AC-17, AC-23, AC-24).
enum SettingsCopy {
    /// The banner title while the daemon cannot be reached.
    static let offlineTitle = "Can’t connect to stacklyd."
    /// The banner message when settings were read earlier in this session.
    static let offlineMessage = "Showing the last known settings."
    /// The label next to a value read earlier in this session.
    static let lastKnown = "last known"
    /// What a row says when it never had a value in this session.
    static let notAvailable = "Not available while stacklyd is offline."
    /// What a location row says about its folder while stacklyd is offline.
    static let unknownWhileOffline = "Unknown while stacklyd is offline"
    /// The line under a row whose value another client changed first.
    static let changedElsewhere = "Changed elsewhere. Try again."

    /// The note of the Autostart new instances row.
    static let autostartNote =
        "Each instance can override this. Applies to instances you create from now on."
    /// The note of the Update channel row (spec 0032 AC-23).
    static let channelNote =
        "Saved now. Used once Stackly can update itself. App updates never touch instance data."
    /// The note of the usage statistics row (spec 0032 AC-23).
    static let telemetryNote =
        "Stackly doesn’t collect usage statistics yet. Your choice is saved for when it does. Diagnostic bundles are never uploaded automatically."

    /// The mode of a row owned by the daemon.
    /// - Parameters:
    ///   - isConnected: Whether the daemon answers now.
    ///   - hasLoaded: Whether the settings were read once in this session.
    static func mode(isConnected: Bool, hasLoaded: Bool) -> SettingsRowMode {
        if isConnected { return .live }
        return hasLoaded ? .lastKnown : .unavailable
    }

    /// The title of a catalog source.
    static func sourceText(_ source: CatalogSourceKind) -> String {
        switch source {
        case .bundled: "Bundled with Stackly"
        case .development: "Development catalog folder"
        case .unrecognized: "Unknown source"
        }
    }
}
