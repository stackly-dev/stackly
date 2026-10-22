import Foundation

/// The shape of a page or section that loads data from the daemon.
///
/// Build it only with ``resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)``
/// and show it with ``StateView``. The order of the shapes is fixed: unsupported, then
/// failed (only before any data loaded), then loading, then empty, then loaded. Once data
/// has loaded, a failed refresh keeps the data and adds a stale note; it never turns the
/// page into `failed`, and the page never shows `empty` before a load succeeded.
public enum ViewState<Value> {
    /// No data has loaded yet. ``StateView`` shows a spinner after 300 ms.
    case loading
    /// The data loaded and is empty, or the filter left nothing.
    case empty(EmptyContent, notes: ContentNotes)
    /// The first load failed and there is no data to show.
    case failed(FailureContent)
    /// The capability behind the page or section is not supported.
    case unsupported(UnsupportedContent)
    /// The data loaded.
    case loaded(Value, notes: ContentNotes)

    /// Resolves the shape from what a screen's view model holds, in the fixed order:
    ///
    /// 1. `unsupported` is set: `.unsupported`.
    /// 2. No `value` and an `error`: `.failed`.
    /// 3. No `value`: `.loading`.
    /// 4. `emptyContent(value)` returns content: `.empty`.
    /// 5. Otherwise: `.loaded`.
    ///
    /// The notes of `.empty` and `.loaded` carry a stale note when a refresh failed
    /// (`value` and `error` both set; it wins over `outdated`) or when `outdated` is set,
    /// plus `partial` and `isRefreshing` as given.
    ///
    /// - Parameters:
    ///   - unsupported: Why the capability is not supported, or `nil` when it is.
    ///   - value: The last data that loaded, before any filter. Keep it through refreshes
    ///     and filter changes.
    ///   - error: The error of the last load, or `nil` after a successful load.
    ///   - lastLoadedAt: When `value` loaded. Set it whenever `value` is set.
    ///   - isRefreshing: Whether a refresh is running.
    ///   - emptyContent: The empty content for `value` (no data when `value` is empty, no
    ///     results when the filtered result is empty), or `nil` when there is something
    ///     to show.
    ///   - outdated: When the source last updated the data and the source's name, when the
    ///     daemon reports the data as out of date. The name may start with a lower case
    ///     letter ("the bundled catalog"): the banner sentence capitalizes its first
    ///     character, so a screen never writes the capital itself.
    ///   - partial: The items that are not ready yet, when only part of the data is ready.
    public static func resolve(
        unsupported: UnsupportedContent? = nil,
        value: Value?,
        error: FailureContent?,
        lastLoadedAt: Date?,
        isRefreshing: Bool = false,
        emptyContent: (Value) -> EmptyContent?,
        outdated: (since: Date, source: String)? = nil,
        partial: PartialNote? = nil
    ) -> ViewState<Value> {
        if let unsupported {
            return .unsupported(unsupported)
        }
        guard let value else {
            if let error {
                return .failed(error)
            }
            return .loading
        }
        let notes = ContentNotes(
            stale: StaleNote(
                error: error,
                lastLoadedAt: lastLoadedAt,
                outdated: outdated
            ),
            partial: partial,
            isRefreshing: isRefreshing
        )
        if let empty = emptyContent(value) {
            return .empty(empty, notes: notes)
        }
        return .loaded(value, notes: notes)
    }

    /// The shape without its content, to compare states and to drive transitions.
    var shape: ViewStateShape {
        switch self {
        case .loading: .loading
        case .empty: .empty
        case .failed: .failed
        case .unsupported: .unsupported
        case .loaded: .loaded
        }
    }
}

/// The five shapes of a ``ViewState`` without their content.
nonisolated enum ViewStateShape: Hashable, Sendable {
    case loading
    case empty
    case failed
    case unsupported
    case loaded
}

/// A button a placeholder or banner shows.
public struct StateAction {
    /// The button title, a verb such as "Retry" or "Browse Catalog".
    public var title: String
    /// Whether the action changes data through the daemon. Such an action is disabled
    /// while ``MutationAvailability`` is `.blocked`.
    public var mutates: Bool
    /// What the button does.
    public var perform: @MainActor () -> Void
    /// Whether the action is running: the button is disabled and shows a small spinner.
    var isBusy: Bool

    /// Creates an action.
    ///
    /// - Parameters:
    ///   - title: The button title, a verb.
    ///   - mutates: Whether the action changes data through the daemon. Copy,
    ///     navigation, Retry, and Open Diagnostics do not.
    ///   - perform: What the button does.
    public init(title: String, mutates: Bool, perform: @escaping @MainActor () -> Void) {
        self.init(title: title, mutates: mutates, isBusy: false, perform: perform)
    }

    init(
        title: String,
        mutates: Bool,
        isBusy: Bool,
        perform: @escaping @MainActor () -> Void
    ) {
        self.title = title
        self.mutates = mutates
        self.isBusy = isBusy
        self.perform = perform
    }

    /// Why the action is disabled under `availability`, or `nil` when it is enabled.
    /// Only an action that mutates is gated.
    func disabledReason(in availability: MutationAvailability) -> String? {
        mutates ? availability.blockedReason : nil
    }
}

/// Which kind of empty a page or section is.
nonisolated public enum EmptyKind: Hashable, Sendable {
    /// The data loaded and holds nothing.
    case noData
    /// The data holds something, but the filter or search left nothing.
    case noResults(query: String)
}

/// What an empty page or section shows: a symbol, a title, a message, and up to two
/// actions.
public struct EmptyContent {
    /// Which kind of empty this is.
    public let kind: EmptyKind
    /// The SF Symbol, drawn in ``StacklyColor/decorative``.
    public let symbol: String
    /// The title.
    public let title: String
    /// The explanation, in ``StacklyColor/textSecondary``.
    public let message: String
    /// The main action, drawn `.borderedProminent` after the secondary one.
    public let primaryAction: StateAction?
    /// The other action, drawn `.bordered`.
    public let secondaryAction: StateAction?

    /// Empty content for data that loaded and holds nothing, for example "No Instances"
    /// with Browse Catalog and New Instance….
    ///
    /// - Parameters:
    ///   - symbol: The SF Symbol of the page or section.
    ///   - title: The title, such as "No Instances".
    ///   - message: What the page holds and how to fill it.
    ///   - primaryAction: The main action, or `nil`.
    ///   - secondaryAction: The other action, or `nil`.
    public static func noData(
        symbol: String,
        title: String,
        message: String,
        primaryAction: StateAction? = nil,
        secondaryAction: StateAction? = nil
    ) -> EmptyContent {
        EmptyContent(
            kind: .noData,
            symbol: symbol,
            title: title,
            message: message,
            primaryAction: primaryAction,
            secondaryAction: secondaryAction
        )
    }

    /// Empty content for a filter or search that left nothing: the `magnifyingglass`
    /// symbol, the title "No Results for “<query>”", and Clear Filters.
    ///
    /// - Parameters:
    ///   - query: The search text or filter name, quoted in the title.
    ///   - message: A suggestion, such as "Check the spelling or try another status."
    ///   - clearFilters: Clears the search and filters.
    public static func noResults(
        query: String,
        message: String,
        clearFilters: @escaping @MainActor () -> Void
    ) -> EmptyContent {
        EmptyContent(
            kind: .noResults(query: query),
            symbol: "magnifyingglass",
            title: "No Results for “\(query)”",
            message: message,
            primaryAction: StateAction(
                title: "Clear Filters", mutates: false, perform: clearFilters),
            secondaryAction: nil
        )
    }
}

/// Why a load failed: the screen's title plus the daemon's message and error code.
nonisolated public struct FailureContent: Hashable, Sendable {
    /// The screen's title, such as "Couldn’t Load Instances".
    public var title: String
    /// The daemon's message, already redacted.
    public var message: String
    /// The daemon's error code, shown monospaced, or `nil`.
    public var code: String?

    /// Creates failure content.
    ///
    /// - Parameters:
    ///   - title: The screen's title.
    ///   - message: The daemon's message.
    ///   - code: The daemon's error code, or `nil`.
    public init(title: String, message: String, code: String? = nil) {
        self.title = title
        self.message = message
        self.code = code
    }
}

/// Why a page or section's capability is not supported.
nonisolated public struct UnsupportedContent: Hashable, Sendable {
    /// The title, such as "Backups Aren’t Supported".
    public var title: String
    /// The reason, from the provider's capabilities.
    public var reason: String

    /// Creates unsupported content.
    ///
    /// - Parameters:
    ///   - title: The title.
    ///   - reason: The reason, shown as the message.
    public init(title: String, reason: String) {
        self.title = title
        self.reason = reason
    }
}

/// What ``StateView`` shows above empty or loaded content: a stale banner, a partial
/// banner, and whether a refresh is running.
public struct ContentNotes {
    /// Why the content may be out of date, or `nil`.
    public var stale: StaleNote?
    /// The items that are not ready yet, or `nil`.
    public var partial: PartialNote?
    /// Whether a refresh is running. The stale banner's Retry is disabled meanwhile.
    public var isRefreshing: Bool

    /// Creates notes.
    ///
    /// - Parameters:
    ///   - stale: Why the content may be out of date, or `nil`.
    ///   - partial: The items that are not ready yet, or `nil`.
    ///   - isRefreshing: Whether a refresh is running.
    public init(stale: StaleNote? = nil, partial: PartialNote? = nil, isRefreshing: Bool = false) {
        self.stale = stale
        self.partial = partial
        self.isRefreshing = isRefreshing
    }
}

/// Part of the data is not ready: a summary and the items that are not ready.
public struct PartialNote {
    /// The summary, such as "5 of 6 services ready".
    public var summary: String
    /// The items that are not ready, each with its reason.
    public var items: [PartialItem]

    /// Creates a partial note.
    ///
    /// - Parameters:
    ///   - summary: The summary.
    ///   - items: The items that are not ready.
    public init(summary: String, items: [PartialItem]) {
        self.summary = summary
        self.items = items
    }
}

/// One item a banner lists: a name, a reason, and an optional action.
public struct PartialItem: Identifiable {
    /// The item's name, such as an instance name. It identifies the item in its list.
    public var name: String
    /// Why the item is not ready.
    public var reason: String
    /// What to do about it, or `nil`.
    public var action: StateAction?

    /// The item's identity: its name.
    public var id: String { name }

    /// Creates an item.
    ///
    /// - Parameters:
    ///   - name: The item's name, unique in its list.
    ///   - reason: Why the item is not ready.
    ///   - action: What to do about it, or `nil`.
    public init(name: String, reason: String, action: StateAction? = nil) {
        self.name = name
        self.reason = reason
        self.action = action
    }
}
