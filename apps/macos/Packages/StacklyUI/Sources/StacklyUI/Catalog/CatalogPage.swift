import StacklyClient
import SwiftUI

/// The Catalog screen (spec 0017, *Layar Catalog (GUI)*): a grid of provider cards with a
/// category picker above it and a search field in the toolbar, and the chosen provider in
/// the inspector.
/// The daemon decides every status and verdict; the page only shows the DTOs. Loading,
/// failure, an unsupported daemon, an empty catalog, no search results, a stale catalog,
/// and providers that failed validation all go through ``ViewState``. The page keeps its
/// provider selection and inspector visibility per window.
public struct CatalogPage: View {
    @SceneStorage("catalog.selection") private var storedSelection = ""
    @SceneStorage("catalog.inspector") private var showsInspector = true
    @State private var filter = CatalogFilter()
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone

    private let catalog: CatalogStore
    private let runtimes: RuntimeStore?
    private let operations: OperationCenter?
    private let waiting: FailureContent?
    private let hasConnection: Bool
    private let reconnect: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void

    /// Creates the page.
    /// - Parameters:
    ///   - catalog: The catalog store.
    ///   - runtimes: The runtime store, for "{n} installed" on a card and the Get column of
    ///     the version matrix, or `nil` for a page without them.
    ///   - operations: The operation center, for the percent of a running install, or `nil`.
    ///   - waiting: What a page without data shows while the daemon cannot be reached
    ///     ("Waiting for stacklyd"), or `nil` when the daemon is reachable or still loading.
    ///   - hasConnection: Whether a connection to the daemon is open: Retry then loads the
    ///     catalog again, else it reconnects.
    ///   - reconnect: Tries to connect now.
    ///   - openDiagnostics: Opens Diagnostics.
    ///   - selection: The provider ID a window with no saved choice starts on; empty
    ///     selects the first provider. Previews and snapshots set it.
    ///   - filter: The filter a new window starts with: All and no search text by default.
    public init(
        catalog: CatalogStore,
        runtimes: RuntimeStore? = nil,
        operations: OperationCenter? = nil,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void,
        selection: String = "",
        filter: CatalogFilter = CatalogFilter()
    ) {
        _storedSelection = SceneStorage(wrappedValue: selection, "catalog.selection")
        _filter = State(initialValue: filter)
        self.catalog = catalog
        self.runtimes = runtimes
        self.operations = operations
        self.waiting = waiting
        self.hasConnection = hasConnection
        self.reconnect = reconnect
        self.openDiagnostics = openDiagnostics
    }

    /// The page.
    public var body: some View {
        let all = catalog.providers
        let visible = all.map(filter.apply(to:)) ?? []
        let selected = CatalogFilter.selection(
            selected: storedSelection.isEmpty ? nil : storedSelection, in: visible)
        VStack(spacing: 0) {
            if all != nil {
                categoryPicker
            }
            StateView(
                state: Self.state(
                    providers: all, info: catalog.info, filter: filter,
                    error: catalog.store.error, unsupported: catalog.store.unsupported,
                    waiting: waiting, loadedAt: catalog.store.loadedAt,
                    isRefreshing: catalog.store.isRefreshing,
                    clearFilters: { filter = CatalogFilter() }),
                retry: retry,
                openDiagnostics: openDiagnostics
            ) { _ in
                grid(visible, selected: selected)
            }
        }
        .stacklyInspector(isPresented: $showsInspector) {
            inspector(selected: visible.first { $0.provider == selected })
        }
        .searchable(text: $filter.query, prompt: "Search providers")

        .pageChrome(
            title: SidebarSection.catalog.title,
            subtitle: catalog.subtitle(locale: locale, calendar: calendar, timeZone: timeZone)
        )

        .onChange(of: selected) { _, new in
            storedSelection = new ?? ""
        }
        .task(
            id: DetailRequest(
                provider: selected, revision: catalog.info?.revision, epoch: catalog.detail.epoch)
        ) {
            if let selected,
                let revision = visible.first(where: { $0.provider == selected })?.revision
            {
                await catalog.detail.load(selected, revision: revision)
            }
        }
    }

    /// The detail a selection asks for, so the task runs again when any part changes,
    /// including a dropped cache.
    private struct DetailRequest: Hashable {
        let provider: String?
        let revision: UInt64?
        let epoch: Int
    }

    /// The segments when they fit, else a menu with the same choices, so no title is cut
    /// in the narrow page column next to the inspector.
    private var categoryPicker: some View {
        ViewThatFits(in: .horizontal) {
            picker.pickerStyle(.segmented).fixedSize(horizontal: true, vertical: false)
            picker.pickerStyle(.menu).fixedSize(horizontal: true, vertical: false)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, Spacing.pageHorizontal)
        .padding(.top, Spacing.s12)
    }

    private var picker: some View {
        Picker("Category", selection: $filter.category) {
            ForEach(CatalogCategory.allCases) { category in
                Text(category.title).tag(category)
            }
        }
        .labelsHidden()

    }

    private func grid(_ providers: [ProviderSummary], selected: String?) -> some View {
        ScrollView {
            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: Self.cardMinWidth), spacing: Spacing.s12)],
                spacing: Spacing.s12
            ) {
                ForEach(providers) { provider in
                    ProviderCard(
                        provider: provider,
                        installedCount: runtimes?.installedCount(software: provider.software) ?? 0,
                        isSelected: provider.provider == selected,
                        select: { storedSelection = provider.provider })
                }
            }
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s12)
        }
    }

    /// The narrowest a card gets before the grid drops a column.
    static let cardMinWidth: CGFloat = 220

    @ViewBuilder
    private func inspector(selected: ProviderSummary?) -> some View {
        if let selected, let info = catalog.info {
            ProviderDetailPanel(
                summary: selected,
                phase: catalog.detail.phase(for: selected.provider, revision: selected.revision),
                host: info.host,
                retry: {
                    Task {
                        await catalog.detail.retry(selected.provider, revision: selected.revision)
                    }
                },
                runtimes: runtimes, operations: operations)
        } else {
            InspectorPlaceholder(
                title: "No Provider Selected", message: "Select a provider to see its details.")
        }
    }

    private func retry() {
        if hasConnection {
            Task { await catalog.store.refresh() }
        } else {
            reconnect()
        }
    }

    // MARK: - State

    /// The state of the page: the fixed order of ``ViewState/resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)``.
    /// The "Waiting for stacklyd" failure counts only while the store has no data. A stale
    /// catalog (180 days old) adds the outdated note with the date it was published, and
    /// providers that failed validation add the partial note.
    static func state(
        providers: [ProviderSummary]?,
        info: CatalogInfo?,
        filter: CatalogFilter,
        error: ErrorBody?,
        unsupported: Bool,
        waiting: FailureContent?,
        loadedAt: Date?,
        isRefreshing: Bool,
        clearFilters: @escaping @MainActor () -> Void
    ) -> ViewState<[ProviderSummary]> {
        let storeFailure = error.map {
            FailureContent(title: "Catalog Unavailable", message: $0.message, code: $0.code)
        }
        return .resolve(
            unsupported: unsupported
                ? UnsupportedContent(
                    title: "Catalog Unavailable",
                    reason: "This version of stacklyd doesn’t provide the catalog. Update Stackly.")
                : nil,
            value: providers,
            error: providers == nil ? (waiting ?? storeFailure) : storeFailure,
            lastLoadedAt: loadedAt,
            isRefreshing: isRefreshing,
            emptyContent: { all in
                if all.isEmpty {
                    return EmptyContent.noData(
                        symbol: SidebarSection.catalog.style.symbol, title: "No Providers",
                        message: "This catalog has no providers.")
                }
                guard filter.isEmpty == false, filter.apply(to: all).isEmpty else { return nil }
                return EmptyContent.noResults(
                    query: filter.description,
                    message: "No providers match “\(filter.description)”.",
                    clearFilters: clearFilters)
            },
            outdated: info.flatMap {
                $0.outdated ? ($0.publishedAt, CatalogCopy.sourceName($0.source)) : nil
            },
            partial: providers.flatMap { partial(info: info, providers: $0) })
    }

    /// "3 providers failed validation." with the IDs of the entries that did.
    static func partial(info: CatalogInfo?, providers: [ProviderSummary]) -> PartialNote? {
        guard let count = info?.invalidCount, count > 0 else { return nil }
        let summary =
            count == 1 ? "1 provider failed validation." : "\(count) providers failed validation."
        let items = providers.filter { $0.state == .invalid }.map {
            PartialItem(name: $0.provider, reason: "Failed validation")
        }
        return PartialNote(summary: summary, items: items)
    }
}
