import Foundation

/// The tabs of the Settings window, in the order they show (spec 0032 AC-13).
enum SettingsTab: String, CaseIterable, Sendable {
    case general
    case instances
    case sources
    case storage
    case updates

    /// The title of the tab.
    var title: String {
        switch self {
        case .general: "General"
        case .instances: "Instances"
        case .sources: "Sources"
        case .storage: "Storage"
        case .updates: "Updates"
        }
    }

    /// The SF Symbol of the tab.
    var symbol: String {
        switch self {
        case .general: "gearshape"
        case .instances: "server.rack"
        case .sources: "shippingbox"
        case .storage: "internaldrive"
        case .updates: "arrow.triangle.2.circlepath"
        }
    }
}

/// Every row the Settings window has now or will have (spec 0032, *Baris Settings*).
enum SettingsRow: String, CaseIterable, Sendable {
    case appearance
    case agentStartup
    case openAtLogin
    case daemon
    case autostartNewInstances
    case notifications
    case catalogSource
    case providers
    case dataLocation
    case logsLocation
    case cacheLocation
    case logRetention
    case updateChannel
    case telemetry
    case cleanUpCache
    case communitySources
    case importProviders
}

/// Which rows the Settings window shows (spec 0032, *Baris Settings*): one entry per row
/// with the feature that owns it. A row whose feature is not built stays out of the window;
/// the feature that builds it changes its entry and nothing else, so no screen hides a row
/// by its own rule.
enum SettingsAvailability {
    /// One row of the table.
    struct Entry: Sendable {
        /// The row.
        let row: SettingsRow
        /// The tab that holds it.
        let tab: SettingsTab
        /// The feature that owns it.
        let owner: String
        /// Whether the row shows today.
        let isBuilt: Bool
    }

    /// The table, one entry per row.
    static let entries: [Entry] = [
        Entry(row: .appearance, tab: .general, owner: "feature 32", isBuilt: true),
        Entry(row: .agentStartup, tab: .general, owner: "features 15 and 31", isBuilt: true),
        Entry(row: .openAtLogin, tab: .general, owner: "feature 32", isBuilt: true),
        Entry(row: .daemon, tab: .general, owner: "features 13 and 30", isBuilt: true),
        Entry(row: .autostartNewInstances, tab: .instances, owner: "feature 32", isBuilt: true),
        Entry(
            row: .notifications, tab: .instances, owner: "feature 32 milestone 4", isBuilt: false),
        Entry(row: .catalogSource, tab: .sources, owner: "feature 17", isBuilt: true),
        Entry(row: .providers, tab: .sources, owner: "feature 17", isBuilt: true),
        Entry(row: .dataLocation, tab: .storage, owner: "feature 32", isBuilt: false),
        Entry(row: .logsLocation, tab: .storage, owner: "feature 32", isBuilt: false),
        Entry(row: .cacheLocation, tab: .storage, owner: "feature 32", isBuilt: false),
        Entry(row: .logRetention, tab: .storage, owner: "feature 27", isBuilt: false),
        Entry(row: .updateChannel, tab: .updates, owner: "feature 32", isBuilt: true),
        Entry(row: .telemetry, tab: .updates, owner: "feature 32", isBuilt: true),
        Entry(row: .cleanUpCache, tab: .storage, owner: "feature 55", isBuilt: false),
        Entry(row: .communitySources, tab: .sources, owner: "feature 37", isBuilt: false),
        Entry(row: .importProviders, tab: .sources, owner: "feature 37", isBuilt: false),
    ]

    /// Whether `row` shows in the window.
    static func isShown(_ row: SettingsRow) -> Bool {
        entries.first { $0.row == row }?.isBuilt ?? false
    }

    /// The rows that show in `tab`, in table order.
    static func rows(in tab: SettingsTab) -> [SettingsRow] {
        entries.filter { $0.tab == tab && $0.isBuilt }.map(\.row)
    }
}
