import Foundation
import StacklyClient
import SwiftUI

/// One row of the provisional instance list: the facts the row shows, already mapped to UI
/// types.
struct InstanceRow: Identifiable, Hashable {
    /// The instance ID.
    let id: UUID
    /// The instance name.
    let name: String
    /// "{provider} {version}", such as "PostgreSQL 17.2.0".
    let detail: String
    /// How the status looks, from the instance status table of spec 0012.
    let appearance: StatusAppearance

    /// The row of an instance.
    init(_ instance: InstanceSummary) {
        id = instance.id
        name = instance.name
        detail = "\(Self.providerTitle(instance.software)) \(instance.runtimeVersion)"
        appearance = StatusAdapter.appearance(of: instance)
    }

    /// What VoiceOver reads for the row: the name, the provider and version, and the status.
    var spokenLabel: String {
        "\(name), \(detail), \(appearance.spokenStatus)"
    }

    /// The display name of a software, such as "PostgreSQL" for `postgresql`. A software
    /// this build does not list gets its identifier with a capital.
    // provisional: F22 (the instance DTO has no provider display name yet)
    static func providerTitle(_ software: String) -> String {
        switch software {
        case "postgresql": "PostgreSQL"
        case "mysql": "MySQL"
        case "nats": "NATS"
        case "clickhouse": "ClickHouse"
        case "valkey": "Valkey"
        case "traefik": "Traefik"
        default: software.prefix(1).uppercased() + software.dropFirst()
        }
    }
}

// provisional: F22
/// The Instances page until feature 22 replaces it: one row per instance with its name,
/// provider and version, and status (spec 0013, *Daftar instance sementara*).
///
/// It has no action while there is no `newInstanceAction` (K17). The list follows the
/// store, so a state change that arrives as an event shows without loading the list again.
public struct ProvisionalInstanceList: View {
    private let store: InstanceStore
    private let waiting: FailureContent?
    private let hasConnection: Bool
    private let reconnect: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void

    /// Creates the list.
    ///
    /// - Parameters:
    ///   - store: The instance store.
    ///   - waiting: What a page without data shows while the daemon cannot be reached
    ///     ("Waiting for stacklyd"), or `nil` when the daemon is reachable or still loading.
    ///   - hasConnection: Whether a connection to the daemon is open: Retry then loads the
    ///     list again, else it reconnects.
    ///   - reconnect: Tries to connect now.
    ///   - openDiagnostics: Opens Diagnostics.
    public init(
        store: InstanceStore,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void
    ) {
        self.store = store
        self.waiting = waiting
        self.hasConnection = hasConnection
        self.reconnect = reconnect
        self.openDiagnostics = openDiagnostics
    }

    /// The list.
    public var body: some View {
        StateView(
            state: Self.state(
                rows: store.loadedCount == nil ? nil : store.instances.map(InstanceRow.init),
                error: store.store.error,
                unsupported: store.store.unsupported,
                waiting: waiting,
                loadedAt: store.store.loadedAt,
                isRefreshing: store.store.isRefreshing),
            retry: retry,
            openDiagnostics: openDiagnostics
        ) { rows in
            InstanceRows(rows: rows)
        }
        .pageChrome(title: SidebarSection.instances.title, subtitle: store.subtitle)
    }

    private func retry() {
        if hasConnection {
            Task { await store.store.refresh() }
        } else {
            reconnect()
        }
    }

    /// The state of the page: the fixed order of ``ViewState/resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)``.
    ///
    /// The "Waiting for stacklyd" failure counts only while the store has no data, so a page
    /// with data does not add a stale note below the window banner.
    static func state(
        rows: [InstanceRow]?,
        error: ErrorBody?,
        unsupported: Bool,
        waiting: FailureContent?,
        loadedAt: Date?,
        isRefreshing: Bool
    ) -> ViewState<[InstanceRow]> {
        let storeFailure = error.map {
            FailureContent(title: "Couldn’t Load Instances", message: $0.message, code: $0.code)
        }
        return .resolve(
            unsupported: unsupported
                ? UnsupportedContent(
                    title: "Instances Aren’t Supported",
                    reason: "This version of stacklyd can’t list instances. Update stacklyd.")
                : nil,
            value: rows,
            error: rows == nil ? (waiting ?? storeFailure) : storeFailure,
            lastLoadedAt: loadedAt,
            isRefreshing: isRefreshing,
            emptyContent: { $0.isEmpty ? Self.emptyContent : nil })
    }

    /// "No Instances", with no action until an action exists (K17).
    static var emptyContent: EmptyContent {
        EmptyContent.noData(
            symbol: SidebarSection.instances.style.symbol,
            title: "No Instances",
            message: "Create one with stackly instance create.")
    }
}

/// The rows of the provisional list, in the order given.
private struct InstanceRows: View {
    let rows: [InstanceRow]

    var body: some View {
        List(rows) { row in
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(row.name)
                        .font(.body.weight(.semibold))
                    Text(row.detail)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
                Spacer(minLength: Spacing.s12)
                StatusLabel(row.appearance, context: .row)
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(row.spokenLabel)
        }
    }
}
