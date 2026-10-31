import StacklyClient
import SwiftUI

/// The Instances page (spec 0022, *Halaman Instances*): a status filter, a native table that
/// sorts by column, a search field in the toolbar, and the chosen instance in the inspector.
/// The page only reads the stores. Start, Stop, and Retry go through the
/// ``InstanceActionModel``; every status, group, and count is a pure function of the
/// summaries. A failed refetch of one instance shows as a note and a detail on its row, and
/// never as an error of the page.
public struct InstancesPage: View {
    /// The table width below which Version and Endpoints are not drawn.
    nonisolated static let versionAndEndpointsMinWidth: CGFloat = 760
    /// The table width below which Provider is not drawn either, so the page column next
    /// to an open inspector still shows Name, Status, and the action.
    nonisolated static let providerMinWidth: CGFloat = 480

    /// The columns the table draws for the width of the page column. It changes only where a
    /// column joins or leaves, so a change of width inside one set never rebuilds the table.
    nonisolated enum ColumnSet: Hashable, Sendable {
        /// Name, Provider, Version, Endpoints, Status, and the action.
        case full
        /// Name, Provider, Status, and the action.
        case withProvider
        /// Name, Status, and the action.
        case compact

        /// The set for a width.
        init(width: CGFloat) {
            if width >= InstancesPage.versionAndEndpointsMinWidth {
                self = .full
            } else if width >= InstancesPage.providerMinWidth {
                self = .withProvider
            } else {
                self = .compact
            }
        }

        var showsProvider: Bool { self != .compact }
        var showsVersionAndEndpoints: Bool { self == .full }
    }

    /// What makes a new table: the columns it draws and whether the inspector narrows it.
    private struct TableIdentity: Hashable {
        let columns: ColumnSet
        let showsInspector: Bool
    }

    @SceneStorage("instances.selection") private var storedSelection = ""
    @SceneStorage("instances.inspector") private var showsInspector = true
    @State private var filter: InstanceFilter
    @State private var query: String
    @State private var sortOrder: [InstanceSortComparator]
    @State private var columns = ColumnSet.full
    @State private var path: [InstanceRoute] = []
    @State private var models: InstanceDetailModels
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.newInstanceAction) private var newInstance
    @Environment(\.duplicateInstanceAction) private var duplicateInstance
    @Environment(\.feedbackCenter) private var feedbackCenter

    private let instances: InstanceStore
    private let catalog: CatalogStore?
    private let actions: InstanceActionModel
    private let operations: OperationCenter?
    private let completedRounds: Int
    private let waiting: FailureContent?
    private let hasConnection: Bool
    private let reconnect: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void

    /// Creates the page.
    /// - Parameters:
    ///   - instances: The instance store.
    ///   - catalog: The catalog, for the display names and tiles of providers, or `nil`.
    ///   - actions: Start, Stop, and Retry.
    ///   - operations: The operation center, to end a pending click after a resnapshot, or
    ///     `nil`.
    ///   - completedRounds: How many snapshot rounds completed; a change ends the pending
    ///     clicks whose operation is gone.
    ///   - waiting: What a page without data shows while the daemon cannot be reached
    ///     ("Waiting for stacklyd"), or `nil` when the daemon is reachable or still loading.
    ///   - hasConnection: Whether a connection to the daemon is open: Retry then loads the
    ///     list again, else it reconnects.
    ///   - reconnect: Tries to connect now.
    ///   - openDiagnostics: Opens Diagnostics.
    ///   - detail: What the detail page and the inspector read from the daemon, or `nil` for
    ///     a window with no daemon, where they show their failed state.
    ///   - opening: The instance whose detail page the page starts on, or `nil` for the table.
    ///     Previews and snapshots set it.
    ///   - selection: The instance ID a window with no saved choice starts on; empty selects
    ///     none. Previews and snapshots set it.
    ///   - showsInspector: Whether a window with no saved choice starts with the inspector
    ///     open.
    ///   - filter: The filter a new window starts with.
    ///   - query: The search text a new window starts with.
    ///   - sortOrder: The order a new window starts with.
    public init(
        instances: InstanceStore,
        catalog: CatalogStore? = nil,
        actions: InstanceActionModel,
        operations: OperationCenter? = nil,
        completedRounds: Int = 0,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void,
        detail: InstanceDetailService? = nil,
        opening: UUID? = nil,
        selection: String = "",
        showsInspector: Bool = true,
        filter: InstanceFilter = .all,
        query: String = "",
        sortOrder: InstanceSortComparator = .default
    ) {
        _storedSelection = SceneStorage(wrappedValue: selection, "instances.selection")
        _showsInspector = SceneStorage(wrappedValue: showsInspector, "instances.inspector")
        _filter = State(initialValue: filter)
        _query = State(initialValue: query)
        _sortOrder = State(initialValue: [sortOrder])
        _models = State(initialValue: InstanceDetailModels(service: detail ?? .disconnected))
        _path = State(initialValue: opening.map { [InstanceRoute($0)] } ?? [])
        self.instances = instances
        self.catalog = catalog
        self.actions = actions
        self.operations = operations
        self.completedRounds = completedRounds
        self.waiting = waiting
        self.hasConnection = hasConnection
        self.reconnect = reconnect
        self.openDiagnostics = openDiagnostics
    }

    private var sort: InstanceSortComparator { sortOrder.first ?? .default }

    private var context: InstanceActionContext {
        InstanceActionContext.make(feedbackCenter: feedbackCenter, navigator: navigator)
    }

    /// The page.
    public var body: some View {
        let all = rows
        let visible = all.map(visibleRows) ?? []
        let selected = selectedRow(in: visible)
        let inView = detailRow(in: all) ?? selected
        let commands = published(selected: selected, inView: inView)
        return NavigationStack(path: $path) {
            GeometryReader { proxy in
                pageContent(all: all, visible: visible, selected: selected)
                    .frame(width: proxy.size.width, height: proxy.size.height, alignment: .top)
            }
            .searchable(text: $query, placement: .toolbar, prompt: "Search")

            .toolbar { newInstanceItem }
            .pageChrome(title: SidebarSection.instances.title, subtitle: instances.subtitle)
            .navigationDestination(for: InstanceRoute.self) { route in
                InstanceDetailPage(
                    id: route.id, instances: instances, catalog: catalog, actions: actions,
                    model: models.model(for: route.id), service: models.detailService,
                    back: { path.removeAll() }
                )
                // On macOS 27 a pushed page does not see the value published around the
                // stack, so the menu would turn every page command off on the detail page.
                // The same commands are published again from inside it.
                .publishesCommands(.page(.instances), commands.actions, checked: commands.checked)
            }
        }
        .stacklyInspector(
            isPresented: $showsInspector,
            help: CommandMap.helpText(
                for: .toggleInspector,
                title: showsInspector ? "Hide Inspector" : "Show Inspector")
        ) {
            inspector(selected: selected)
        }
        .publishesCommands(.page(.instances), commands.actions, checked: commands.checked)
        // The navigator keeps a request while Instances shows, so a page that SwiftUI builds
        // again after the first one applied it applies it too.
        .onChange(of: navigator?.instancesRequest, initial: true) { _, request in
            apply(request)
        }
        .onChange(of: navigator?.instanceRequest, initial: true) { _, request in
            open(request)
        }
        .onChange(of: navigator?.instancesResets) {
            path.removeAll()
        }
        .task(id: completedRounds) {
            await reconcile()
        }
    }

    private func pageContent(
        all: [InstanceRowModel]?, visible: [InstanceRowModel], selected: InstanceRowModel?
    ) -> some View {
        VStack(spacing: 0) {
            if all?.isEmpty == false {
                filterPicker
            }
            StateView(
                state: state(all: all, visibleCount: visible.count),
                retry: retry,
                openDiagnostics: openDiagnostics
            ) { _ in
                table(visible, selected: selected)
            }
        }
    }

    private func state(all: [InstanceRowModel]?, visibleCount: Int) -> ViewState<
        [InstanceRowModel]
    > {
        Self.state(
            rows: all, visibleCount: visibleCount, filter: filter, query: query,
            error: instances.store.error, unsupported: instances.store.unsupported,
            waiting: waiting, loadedAt: instances.store.loadedAt,
            isRefreshing: instances.store.isRefreshing,
            partial: partialNote(rows: all ?? []),
            browseCatalog: { navigator?.show(.catalog) },
            newInstance: newInstance.map { $0.stacklyAction(title: "New Instance…") },
            clearFilters: clearFilters)
    }

    @ToolbarContentBuilder private var newInstanceItem: some ToolbarContent {
        if let newInstance {
            ToolbarItem(placement: .primaryAction) {
                ActionButton(newInstance.stacklyAction(title: "New Instance"), showsReason: false)
                    .help(CommandMap.helpText(for: .newInstance))
            }
        }
    }

    private func reconcile() async {
        let names = Dictionary(
            instances.instances.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first })
        models.keep(Set(instances.instances.map(\.id)))
        await actions.reconcile(
            activeOperations: Set((operations?.active ?? []).map(\.id)), names: names,
            context: context)
    }

    // MARK: - Rows

    /// The rows of every instance, or `nil` before the first snapshot.
    private var rows: [InstanceRowModel]? {
        guard instances.loadedCount != nil else { return nil }
        return InstanceRowModel.rows(
            of: instances.instances, catalog: catalog?.providers,
            failures: Set(instances.store.itemFailures.keys))
    }

    /// The rows the filter and the search let through, in the order of the table.
    private func visibleRows(_ all: [InstanceRowModel]) -> [InstanceRowModel] {
        let matching = all.filter {
            filter.includes($0.group) && InstanceSearch.matches($0, query: query)
        }
        return InstanceRowModel.sorted(matching, by: sort)
    }

    private var selectionID: UUID? {
        storedSelection.isEmpty ? nil : UUID(uuidString: storedSelection)
    }

    private func selectedRow(in visible: [InstanceRowModel]) -> InstanceRowModel? {
        visible.first { $0.id == selectionID }
    }

    // MARK: - Filter

    /// The segments when they fit, else a menu with the same choices.
    private var filterPicker: some View {
        ViewThatFits(in: .horizontal) {
            picker.pickerStyle(.segmented).fixedSize(horizontal: true, vertical: false)
            picker.pickerStyle(.menu).fixedSize(horizontal: true, vertical: false)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, Spacing.pageHorizontal)
        .padding(.vertical, Spacing.s12)
    }

    private var picker: some View {
        Picker("Status", selection: $filter) {
            ForEach(InstanceFilter.allCases) { filter in
                Text(filter.title).tag(filter)
            }
        }
        .labelsHidden()

    }

    private func clearFilters() {
        filter = .all
        query = ""
    }

    // MARK: - Table

    /// The table, sized to the space the page gives it. A table that is not sized this way
    /// keeps the height of all its rows when a note stands above it, and the width it was
    /// made with when the inspector narrows the page.
    private func table(_ visible: [InstanceRowModel], selected: InstanceRowModel?) -> some View {
        GeometryReader { proxy in
            rowsTable(visible)
                .frame(width: proxy.size.width, height: proxy.size.height)
                .onChange(of: ColumnSet(width: proxy.size.width), initial: true) { _, new in
                    columns = new
                }
        }
    }

    private func rowsTable(_ visible: [InstanceRowModel]) -> some View {
        Table(visible, selection: selectionBinding(visible: visible), sortOrder: $sortOrder) {
            TableColumn("Name", sortUsing: InstanceSortComparator(.name)) { row in
                HStack(spacing: Spacing.s8) {
                    ProviderTile(row.tile, size: ProviderTile.smallSize)
                    Text(row.name)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                .help(row.name)

            }
            .width(min: 120, ideal: 180)
            if columns.showsProvider {
                TableColumn("Provider", sortUsing: InstanceSortComparator(.provider)) { row in
                    Text(row.providerTitle).lineLimit(1).help(row.providerTitle)
                }
                .width(min: 90, ideal: 130, max: 200)
            }
            if columns.showsVersionAndEndpoints {
                TableColumn("Version", sortUsing: InstanceSortComparator(.version)) { row in
                    Text(row.version ?? "—").lineLimit(1).help(row.version ?? "No version")
                }
                .width(min: 60, ideal: 80, max: 110)
                TableColumn("Endpoints", sortUsing: InstanceSortComparator(.endpoints)) { row in
                    Text(row.ports).lineLimit(1).help(row.ports)
                }
                .width(min: 80, ideal: 110, max: 160)
            }
            TableColumn("Status", sortUsing: InstanceSortComparator(.status)) { row in
                StatusLabel(row.appearance, context: .row)
                    .help(
                        row.appearance.detail.map { "\(row.appearance.label): \($0.text)" }
                            ?? row.appearance.label)
            }
            .width(min: 110, ideal: 190, max: 280)
            TableColumn("") { row in
                rowAction(row)
            }
            .width(min: 100, ideal: 124, max: 150)
        }
        // A table keeps the column widths it was made with, so a new set of columns, or a page
        // column that the inspector narrowed, makes a new table.
        .id(TableIdentity(columns: columns, showsInspector: showsInspector))
        .contextMenu(forSelectionType: UUID.self) { ids in
            contextMenu(for: ids, in: visible)
        } primaryAction: { ids in
            if let id = ids.first { navigator?.openInstance(id) }
        }

    }

    private func selectionBinding(visible: [InstanceRowModel]) -> Binding<UUID?> {
        Binding {
            visible.first { $0.id == selectionID }?.id
        } set: { newValue in
            // The table reports nothing selected when the chosen row is filtered out; the
            // choice stays and comes back with the row.
            if newValue == nil, selectionID != nil,
                visible.contains(where: { $0.id == selectionID }) == false
            {
                return
            }
            storedSelection = newValue?.uuidString.lowercased() ?? ""
        }
    }

    private func rowAction(_ row: InstanceRowModel) -> some View {
        let action = actions.stacklyAction(for: row.summary, context: context)
        return ActionButton(action, showsReason: false)
            .controlSize(.small)
            .help(action.disabledReason ?? action.title)
    }

    @ViewBuilder
    private func contextMenu(for ids: Set<UUID>, in visible: [InstanceRowModel]) -> some View {
        if let id = ids.first, let row = visible.first(where: { $0.id == id }) {
            ActionMenuItems([
                [actions.stacklyAction(for: row.summary, context: context)],
                [
                    copyAction("copyName", "Copy Name", row.name, what: "Name"),
                    copyAction(
                        "copyID", "Copy ID", row.summary.id.uuidString.lowercased(), what: "ID"),
                ],
            ])
        }
    }

    private func copyAction(_ id: String, _ title: String, _ text: String, what: String)
        -> StacklyAction
    {
        StacklyAction(
            id: id, title: title, mutates: false,
            perform: {
                let confirmation = CopyConfirmation()
                Task { await confirmation.copy(text) }
            })
    }

    // MARK: - Inspector

    @ViewBuilder
    private func inspector(selected: InstanceRowModel?) -> some View {
        if let selected {
            InstanceInspector(
                row: selected,
                action: actions.stacklyAction(for: selected.summary, context: context),
                isPrimary: Self.isPrimary(actions.primaryAction(for: selected.summary)),
                open: openAction(for: selected),
                model: models.model(for: selected.id),
                secrets: models.detailService.secrets)
        } else {
            InspectorPlaceholder(
                title: "No Selection",
                message: "Select an instance to see its details. Double-click a row to open it.")
        }
    }

    /// Open: the detail page of the instance (spec 0021, AC-15).
    private func openAction(for row: InstanceRowModel) -> StacklyAction {
        StacklyAction(
            id: CommandID.openInstance.rawValue, title: "Open", mutates: false,
            perform: { navigator?.openInstance(row.id) })
    }

    /// Start and Retry are the main action of the place; Stop and a busy button are not.
    static func isPrimary(_ action: InstanceAction) -> Bool {
        action.kind == .start || action.kind == .retry
    }

    // MARK: - Commands

    /// The row of the instance whose detail page is open, or `nil` while the table shows.
    private func detailRow(in all: [InstanceRowModel]?) -> InstanceRowModel? {
        guard let id = path.last?.id else { return nil }
        return all?.first { $0.id == id }
    }

    /// What the page publishes to the menu bar. The instance in view is the one whose detail
    /// page is open, else the selected row; Open needs a selected row and a table, Back needs
    /// a detail page (spec 0069 AC-11 and AC-12).
    private func published(selected: InstanceRowModel?, inView: InstanceRowModel?) -> (
        actions: [CommandID: StacklyAction], checked: Set<CommandID>
    ) {
        let target = inView.map { row in
            (
                action: actions.stacklyAction(for: row.summary, context: context),
                verb: actions.primaryAction(for: row.summary).commandVerb
            )
        }
        let isDetailOpen = path.isEmpty == false
        var open: StacklyAction?
        if isDetailOpen == false, let selected {
            open = openAction(for: selected)
        }
        var back: (@MainActor () -> Void)?
        if isDetailOpen {
            back = { path.removeAll() }
        }
        var copyConnection: StacklyAction?
        if let inView {
            copyConnection = ConnectionCopier.action(
                detail: models.model(for: inView.id).detail,
                secrets: models.detailService.secrets,
                report: { feedbackCenter?.show($0) })
        }
        var duplicate: StacklyAction?
        if let inView, let duplicateInstance {
            duplicate = duplicateInstance.stacklyAction(
                for: DuplicateSourceRef(id: inView.id, name: inView.name))
        }
        return InstancesCommandActions.publish(
            showsInspector: showsInspector,
            toggleInspector: { showsInspector.toggle() },
            sort: sort,
            setSort: { sortOrder = [$0] },

            target: target, open: open, back: back, copyConnection: copyConnection,
            duplicate: duplicate)
    }

    private func apply(_ request: InstancesRequest?) {
        guard let request else { return }
        filter = request.filter
        query = ""
        if let id = request.select {
            storedSelection = id.uuidString.lowercased()
            showsInspector = true
        }
    }

    /// Opens the detail page of an instance and keeps its row selected, so Back returns to
    /// the table with the row chosen.
    private func open(_ request: InstanceRequest?) {
        guard let request else { return }
        storedSelection = request.instance.uuidString.lowercased()
        // A request comes from another page, so the detail page is there at once, not pushed
        // with the animation of a tap.
        var transaction = Transaction()
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            path = [InstanceRoute(request.instance)]
        }
    }

    private func retry() {
        if hasConnection {
            Task { await instances.store.refresh() }
        } else {
            reconnect()
        }
    }

    // MARK: - State

    private func partialNote(rows: [InstanceRowModel]) -> PartialNote? {
        Self.partial(
            failures: instances.store.itemFailures,
            names: Dictionary(
                rows.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first }),
            retry: { id in Task { await instances.store.retry(id) } })
    }

    /// "Couldn’t refresh 2 instances." with one item per instance and its Retry.
    static func partial(
        failures: [UUID: ItemFailure], names: [UUID: String],
        retry: @escaping @MainActor (UUID) -> Void
    ) -> PartialNote? {
        guard failures.isEmpty == false else { return nil }
        let summary =
            failures.count == 1
            ? "Couldn’t refresh 1 instance." : "Couldn’t refresh \(failures.count) instances."
        let items = failures.map { id, failure in
            PartialItem(
                name: names[id] ?? id.uuidString.lowercased(),
                reason: DaemonText.sentence(failure.error.message),
                action: StateAction(title: "Retry", mutates: false, perform: { retry(id) }))
        }
        .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        return PartialNote(summary: summary, items: items)
    }

    /// The state of the page: the fixed order of ``ViewState/resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)``.
    /// The "Waiting for stacklyd" failure counts only while the store has no data.
    static func state(
        rows: [InstanceRowModel]?,
        visibleCount: Int,
        filter: InstanceFilter,
        query: String,
        error: ErrorBody?,
        unsupported: Bool,
        waiting: FailureContent?,
        loadedAt: Date?,
        isRefreshing: Bool,
        partial: PartialNote?,
        browseCatalog: @escaping @MainActor () -> Void,
        newInstance: StacklyAction?,
        clearFilters: @escaping @MainActor () -> Void
    ) -> ViewState<[InstanceRowModel]> {
        let storeFailure = error.map {
            FailureContent(title: "Couldn’t Load Instances", message: $0.message, code: $0.code)
        }
        return .resolve(
            unsupported: unsupported
                ? UnsupportedContent(
                    title: "Instances Unavailable",
                    reason: "This version of stacklyd doesn’t list instances. Update Stackly.")
                : nil,
            value: rows,
            error: rows == nil ? (waiting ?? storeFailure) : storeFailure,
            lastLoadedAt: loadedAt,
            isRefreshing: isRefreshing,
            emptyContent: { all in
                if all.isEmpty {
                    return noInstances(browseCatalog: browseCatalog, newInstance: newInstance)
                }
                guard visibleCount == 0 else { return nil }
                let trimmed = query.trimmingCharacters(in: .whitespaces)
                return EmptyContent.noResults(
                    query: trimmed.isEmpty ? filter.title : trimmed,
                    message: "Check the spelling or try another status.",
                    clearFilters: clearFilters)
            },
            partial: partial)
    }

    /// "No Instances", with Browse Catalog and, once feature 20 fills it, New Instance….
    static func noInstances(
        browseCatalog: @escaping @MainActor () -> Void, newInstance: StacklyAction?
    ) -> EmptyContent {
        EmptyContent.noData(
            symbol: SidebarSection.instances.style.symbol,
            title: "No Instances",
            message:
                "Each instance gets its own data directory, port and secret. Start by installing a runtime.",
            primaryAction: StateAction(
                title: "Browse Catalog", mutates: false, perform: browseCatalog),
            secondaryAction: newInstance.map { action in
                StateAction(title: action.title, mutates: true, perform: action.perform)
            })
    }
}

#if DEBUG
    /// Loads the fixture stores, then shows the page: a preview cannot await.
    private struct InstancesPagePreview: View {
        let selection: String
        let showsInspector: Bool
        @State private var rig = PageRig()
        @State private var isLoaded = false

        var body: some View {
            Group {
                if isLoaded {
                    rig.instancesPage(selection: selection, showsInspector: showsInspector)
                } else {
                    Color.clear
                }
            }
            .environment(\.fixtureNow, FixtureClock.now)
            .task {
                await rig.loadInstances()
                isLoaded = true
            }
        }
    }

    private let previewSelection = FixtureID.uuid(kind: FixtureID.instance, number: 5)
        .uuidString.lowercased()

    #Preview("Light") {
        InstancesPagePreview(selection: previewSelection, showsInspector: true)
            .frame(width: 1044, height: 748)
            .preferredColorScheme(.light)
    }

    #Preview("Dark") {
        InstancesPagePreview(selection: previewSelection, showsInspector: true)
            .frame(width: 1044, height: 748)
            .preferredColorScheme(.dark)
    }

    #Preview("640 pt, inspector closed") {
        InstancesPagePreview(selection: "", showsInspector: false)
            .frame(width: 640, height: 748)
            .preferredColorScheme(.light)
    }

    #Preview("640 pt, inspector open") {
        InstancesPagePreview(selection: previewSelection, showsInspector: true)
            .frame(width: 640, height: 748)
            .preferredColorScheme(.light)
    }

    #Preview("384 pt page column with the inspector") {
        InstancesPagePreview(selection: previewSelection, showsInspector: true)
            .frame(width: 664, height: 748)
            .preferredColorScheme(.light)
    }
#endif
