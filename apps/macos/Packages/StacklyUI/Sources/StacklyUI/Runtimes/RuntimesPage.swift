import StacklyClient
import SwiftUI

/// The Runtimes page (spec 0018, *Halaman Runtimes*): the installs that have not ended, a
/// native table of the managed runtimes with a locked Remove button, and the chosen runtime
/// in the inspector.
///
/// The page only reads the stores. Every state, count, and verdict is a pure function of the
/// summaries the daemon sent. Loading, failure, an unsupported daemon, an empty list, a
/// stale list, and a runtime that could not be read again all go through ``ViewState``.
///
/// In the 384 pt page column next to an open inspector the table drops the Source and
/// Digest columns, whose values stay in the inspector and in what VoiceOver reads; the
/// text wraps and nothing is cut.
public struct RuntimesPage: View {
    /// The table width below which Source and Digest are not drawn (spec 0018, 2026-10-21).
    nonisolated static let sourceAndDigestMinWidth: CGFloat = 560
    /// The narrowest the content is measured at: the page column next to an open inspector
    /// in the smallest window (spec 0018, 2026-10-21). The split view also asks for the
    /// minimum size at a width of zero, where wrapped text would be one word to a line and
    /// push the window taller than the screen.
    nonisolated static let minimumWidth: CGFloat = 384

    /// What makes a new table: the columns it draws and whether the inspector narrows it.
    private struct TableIdentity: Hashable {
        let showsSourceAndDigest: Bool
        let showsInspector: Bool
    }

    /// The detail a selection asks for, so the task runs again when either changes.
    private struct DetailRequest: Hashable {
        let runtime: UUID?
        let revision: UInt64?
    }

    @SceneStorage("runtimes.selection") private var storedSelection = ""
    @SceneStorage("runtimes.inspector") private var showsInspector = true
    @State private var tableSize: CGSize?
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.feedbackCenter) private var feedbackCenter

    private let runtimes: RuntimeStore
    private let catalog: CatalogStore?
    private let operations: OperationCenter?
    private let waiting: FailureContent?
    private let hasConnection: Bool
    private let reconnect: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void

    /// Creates the page.
    ///
    /// - Parameters:
    ///   - runtimes: The runtime store.
    ///   - catalog: The catalog, for the tiles of providers, or `nil`.
    ///   - operations: The operation center, for the progress of the installs, or `nil`.
    ///   - waiting: What a page without data shows while the daemon cannot be reached
    ///     ("Waiting for stacklyd"), or `nil` when the daemon is reachable or still loading.
    ///   - hasConnection: Whether a connection to the daemon is open: Retry then loads the
    ///     list again, else it reconnects.
    ///   - reconnect: Tries to connect now.
    ///   - openDiagnostics: Opens Diagnostics.
    ///   - selection: The runtime ID a window with no saved choice starts on; empty selects
    ///     none. Previews and snapshots set it.
    ///   - showsInspector: Whether a window with no saved choice starts with the inspector
    ///     open.
    public init(
        runtimes: RuntimeStore,
        catalog: CatalogStore? = nil,
        operations: OperationCenter? = nil,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void,
        selection: String = "",
        showsInspector: Bool = true
    ) {
        _storedSelection = SceneStorage(wrappedValue: selection, "runtimes.selection")
        _showsInspector = SceneStorage(wrappedValue: showsInspector, "runtimes.inspector")
        self.runtimes = runtimes
        self.catalog = catalog
        self.operations = operations
        self.waiting = waiting
        self.hasConnection = hasConnection
        self.reconnect = reconnect
        self.openDiagnostics = openDiagnostics
    }

    private var context: InstanceActionContext {
        InstanceActionContext.make(feedbackCenter: feedbackCenter, navigator: navigator)
    }

    /// The page.
    public var body: some View {
        let all = rows
        let selected = all?.first { $0.id == selectionID }
        StateView(
            state: state(all: all),
            retry: retry,
            openDiagnostics: openDiagnostics
        ) { rows in
            content(rows, selected: selected)
        }
        // provisional: F67 (a `.stacklyInspector` fix replaces this minimum width: the split
        // view measures the page at a width of zero, where its notes and text wrap per word)
        // The notes above the content wrap at the width the split view measures them at too.
        .frame(minWidth: Self.minimumWidth, maxWidth: .infinity, maxHeight: .infinity)
        .stacklyInspector(isPresented: $showsInspector) {
            inspector(selected: selected)
        }
        .toolbar { browseCatalogItem }
        .pageChrome(title: SidebarSection.runtimes.title, subtitle: runtimes.subtitle)
        .task(id: DetailRequest(runtime: selected?.id, revision: selected?.summary.revision)) {
            if let selected {
                await runtimes.detail.load(selected.id, revision: selected.summary.revision)
            }
        }
    }

    @ToolbarContentBuilder private var browseCatalogItem: some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            Button("Browse Catalog") { navigator?.show(.catalog) }
                .help("Show the catalog of providers")
                .accessibilityIdentifier(AccessibilityID.Action.button("browseCatalog"))
        }
    }

    // MARK: - Rows

    /// The rows of every runtime, or `nil` before the first snapshot.
    private var rows: [RuntimeRowModel]? {
        guard runtimes.loadedCount != nil else { return nil }
        return RuntimeRowModel.rows(of: runtimes.runtimes, catalog: catalog?.providers)
    }

    private var selectionID: UUID? {
        storedSelection.isEmpty ? nil : UUID(uuidString: storedSelection)
    }

    // MARK: - Content

    private func content(_ rows: [RuntimeRowModel], selected: RuntimeRowModel?) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s16) {
            if runtimes.installing.isEmpty == false {
                installingSection
                    .padding(.horizontal, Spacing.pageHorizontal)
                    .padding(.top, Spacing.s12)
            }
            if rows.isEmpty == false {
                table(rows)
            } else {
                Spacer(minLength: 0)
            }
            Text(RuntimeCopy.footnote)
                .font(.footnote)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.bottom, Spacing.s12)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }

    private var installingSection: some View {
        CardSection("Installing") {
            ForEach(runtimes.installing) { install in
                let operation = operations?.store.items[install.operationId]
                InstallingRow(
                    install: install,
                    tile: RuntimeRowModel.tile(
                        software: install.software, name: install.displayName,
                        provider: install.provider, catalog: catalog?.providers),
                    progress: RuntimeInstallProgress.value(operation: operation),
                    cancel: runtimes.cancelAction(
                        for: install, stage: operation?.stage, context: context))
            }
        }
        .accessibilityIdentifier(AccessibilityID.Runtimes.installing)
    }

    // MARK: - Table

    /// The table, sized to the space the page gives it. The space is measured on a view that
    /// stands in its place, and the table is drawn over it at that size: a table that is
    /// not sized this way keeps the height of all its rows. The measuring view is not a
    /// `GeometryReader`, which holds its content again and drops the system settings the
    /// inspector already drops once.
    private func table(_ rows: [RuntimeRowModel]) -> some View {
        Color.clear
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .onGeometryChange(for: CGSize.self) {
                $0.size
            } action: {
                tableSize = $0
            }
            .overlay(alignment: .topLeading) {
                if let tableSize {
                    rowsTable(
                        rows, showsSourceAndDigest: Self.showsSourceAndDigest(tableSize.width)
                    )
                    .frame(width: tableSize.width, height: tableSize.height)
                }
            }
    }

    /// Whether the table is wide enough for Source and Digest.
    nonisolated static func showsSourceAndDigest(_ width: CGFloat) -> Bool {
        width >= sourceAndDigestMinWidth
    }

    private func rowsTable(_ rows: [RuntimeRowModel], showsSourceAndDigest: Bool) -> some View {
        Table(rows, selection: selectionBinding(rows)) {
            TableColumn("Runtime") { row in
                HStack(spacing: Spacing.s8) {
                    ProviderTile(row.tile, size: ProviderTile.rowSize)
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Text(row.name)
                            .fixedSize(horizontal: false, vertical: true)
                        Text(row.secondLine)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(row.spokenLabel)
                .accessibilityIdentifier(AccessibilityID.Runtimes.row(row.id))
            }
            .width(min: 150, ideal: 170)
            if showsSourceAndDigest {
                TableColumn("Source") { row in
                    Text(row.source)
                }
                .width(min: 60, ideal: 70, max: 90)
                TableColumn("Digest") { row in
                    Text(row.digest)
                        .font(.callout.monospaced())
                        .lineLimit(1)
                        .help(row.summary.artifactSha256.map { "sha256:\($0)" } ?? "No digest")
                }
                .width(min: 150, ideal: 156, max: 190)
            }
            TableColumn("Used By") { row in
                Text(row.usedBy)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .width(min: 70, ideal: 100)
            TableColumn("") { row in
                removeButton(row)
            }
            .width(min: 36, ideal: 40, max: 48)
        }
        // A table keeps the column widths it was made with, so a new set of columns, or a
        // page column that the inspector narrowed, makes a new table.
        .id(
            TableIdentity(
                showsSourceAndDigest: showsSourceAndDigest, showsInspector: showsInspector)
        )
        .contextMenu(forSelectionType: UUID.self) { ids in
            contextMenu(for: ids, in: rows)
        } primaryAction: { ids in
            if let id = ids.first {
                storedSelection = id.uuidString.lowercased()
                showsInspector = true
            }
        }
        .accessibilityIdentifier(AccessibilityID.Runtimes.table)
    }

    private func selectionBinding(_ rows: [RuntimeRowModel]) -> Binding<UUID?> {
        Binding {
            rows.first { $0.id == selectionID }?.id
        } set: { newValue in
            storedSelection = newValue?.uuidString.lowercased() ?? ""
        }
    }

    /// The Remove button, locked: it says why in its help and to VoiceOver.
    private func removeButton(_ row: RuntimeRowModel) -> some View {
        let action = StacklyAction(
            id: "removeRuntime", title: "Remove…", symbol: "trash",
            role: .destructive(
                ActionConfirmation(
                    title: "Remove “\(row.name)”?",
                    message: "Its files are deleted. Instances that used it keep their data.",
                    actionTitle: "Remove")),
            mutates: true, disabledReason: row.removeReason, perform: {})
        return ActionButton(action, showsReason: false)
            .labelStyle(.iconOnly)
            .controlSize(.small)
            .accessibilityLabel("Remove \(row.name)")
            .accessibilityHint(row.removeReason)
    }

    @ViewBuilder
    private func contextMenu(for ids: Set<UUID>, in rows: [RuntimeRowModel]) -> some View {
        if let id = ids.first, let row = rows.first(where: { $0.id == id }) {
            ActionMenuItems([
                [
                    copyAction(
                        "copyDigest", "Copy Digest",
                        row.summary.artifactSha256.map { "sha256:\($0)" },
                        what: "Digest"),
                    copyAction("copyName", "Copy Name", row.name, what: "Name"),
                    copyAction(
                        "copyID", "Copy ID", row.summary.id.uuidString.lowercased(), what: "ID"),
                ]
            ])
        }
    }

    private func copyAction(_ id: String, _ title: String, _ text: String?, what: String)
        -> StacklyAction
    {
        StacklyAction(
            id: id, title: title, mutates: false,
            disabledReason: text == nil ? "This runtime has no digest" : nil,
            perform: {
                guard let text else { return }
                let confirmation = CopyConfirmation()
                Task { await confirmation.copy(text, announcement: "\(what) copied.") }
            })
    }

    // MARK: - Inspector

    @ViewBuilder
    private func inspector(selected: RuntimeRowModel?) -> some View {
        if let selected {
            RuntimeInspector(
                row: selected,
                phase: runtimes.detail.phase(
                    for: selected.id, revision: selected.summary.revision),
                retry: {
                    Task {
                        await runtimes.detail.retry(
                            selected.id, revision: selected.summary.revision)
                    }
                })
        } else {
            InspectorPlaceholder(
                title: "No Selection", message: "Select a runtime to see its details.")
        }
    }

    private func retry() {
        if hasConnection {
            Task { await runtimes.store.refresh() }
        } else {
            reconnect()
        }
    }

    // MARK: - State

    private func state(all: [RuntimeRowModel]?) -> ViewState<[RuntimeRowModel]> {
        Self.state(
            rows: all, installing: runtimes.installing, error: runtimes.store.error,
            unsupported: runtimes.store.unsupported, waiting: waiting,
            loadedAt: runtimes.store.loadedAt, isRefreshing: runtimes.store.isRefreshing,
            partial: partialNote(rows: all ?? []),
            browseCatalog: { navigator?.show(.catalog) })
    }

    private func partialNote(rows: [RuntimeRowModel]) -> PartialNote? {
        Self.partial(
            failures: runtimes.store.itemFailures,
            names: Dictionary(
                rows.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first }),
            retry: { id in Task { await runtimes.store.retry(id) } })
    }

    /// "Couldn’t refresh 2 runtimes." with one item per runtime and its Retry.
    static func partial(
        failures: [UUID: ItemFailure], names: [UUID: String],
        retry: @escaping @MainActor (UUID) -> Void
    ) -> PartialNote? {
        guard failures.isEmpty == false else { return nil }
        let summary =
            failures.count == 1
            ? "Couldn’t refresh 1 runtime." : "Couldn’t refresh \(failures.count) runtimes."
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
    ///
    /// The "Waiting for stacklyd" failure counts only while the store has no data. An empty
    /// list with an install running is not empty: the Installing section shows.
    static func state(
        rows: [RuntimeRowModel]?,
        installing: [InstallingRuntime],
        error: ErrorBody?,
        unsupported: Bool,
        waiting: FailureContent?,
        loadedAt: Date?,
        isRefreshing: Bool,
        partial: PartialNote?,
        browseCatalog: @escaping @MainActor () -> Void
    ) -> ViewState<[RuntimeRowModel]> {
        let storeFailure = error.map {
            FailureContent(title: "Couldn’t Load Runtimes", message: $0.message, code: $0.code)
        }
        return .resolve(
            unsupported: unsupported
                ? UnsupportedContent(
                    title: "Runtimes Unavailable",
                    reason: "This version of stacklyd doesn’t manage runtimes. Update Stackly.")
                : nil,
            value: rows,
            error: rows == nil ? (waiting ?? storeFailure) : storeFailure,
            lastLoadedAt: loadedAt,
            isRefreshing: isRefreshing,
            emptyContent: { all in
                guard all.isEmpty, installing.isEmpty else { return nil }
                return noRuntimes(browseCatalog: browseCatalog)
            },
            partial: partial)
    }

    /// "No Runtimes", with Browse Catalog.
    static func noRuntimes(browseCatalog: @escaping @MainActor () -> Void) -> EmptyContent {
        EmptyContent.noData(
            symbol: SidebarSection.runtimes.style.symbol,
            title: "No Runtimes",
            message: RuntimeCopy.emptyMessage,
            primaryAction: StateAction(
                title: "Browse Catalog", mutates: false, perform: browseCatalog))
    }
}

#if DEBUG
    /// Loads the fixture stores, then shows the page: a preview cannot await.
    private struct RuntimesPagePreview: View {
        let selection: String
        let showsInspector: Bool
        @State private var rig = RuntimeRig()
        @State private var isLoaded = false

        var body: some View {
            Group {
                if isLoaded {
                    rig.runtimesPage(selection: selection, showsInspector: showsInspector)
                } else {
                    Color.clear
                }
            }
            .environment(\.fixtureNow, FixtureClock.now)
            .task {
                await rig.load()
                isLoaded = true
            }
        }
    }

    private let previewSelection = FixtureRuntime.summaries[0].id.uuidString.lowercased()

    #Preview("Light") {
        RuntimesPagePreview(selection: previewSelection, showsInspector: true)
            .frame(width: 1044, height: 748)
            .preferredColorScheme(.light)
    }

    #Preview("Dark") {
        RuntimesPagePreview(selection: previewSelection, showsInspector: true)
            .frame(width: 1044, height: 748)
            .preferredColorScheme(.dark)
    }

    #Preview("640 pt, inspector closed") {
        RuntimesPagePreview(selection: "", showsInspector: false)
            .frame(width: 640, height: 748)
            .preferredColorScheme(.light)
    }

    #Preview("384 pt page column with the inspector") {
        RuntimesPagePreview(selection: previewSelection, showsInspector: true)
            .frame(width: 664, height: 748)
            .preferredColorScheme(.light)
    }
#endif
