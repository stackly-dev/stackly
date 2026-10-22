import StacklyClient
import SwiftUI

/// The Overview (spec 0022, *Halaman Overview*): four cards, Needs Attention, the first
/// instances, and the operations that run, in two columns when the page is wide enough.
///
/// Every number comes from the stores, through pure functions: ``InstanceCounts``,
/// ``AttentionRules``, and ``OperationTitle``. No number shows before its store loaded, and
/// a section that fails on its own never changes the shape of the page.
public struct OverviewPage: View {
    /// The content width from which the page uses two columns.
    static let twoColumnMinWidth: CGFloat = 820

    @Environment(\.shellNavigator) private var navigator
    @Environment(\.newInstanceAction) private var newInstance
    @Environment(\.feedbackCenter) private var feedbackCenter
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    private let instances: InstanceStore
    private let catalog: CatalogStore?
    private let runtimes: RuntimeStore?
    private let operations: OperationCenter
    private let capacity: CapacityModel
    private let actions: InstanceActionModel
    private let isConnected: Bool
    private let completedRounds: Int
    private let waiting: FailureContent?
    private let hasConnection: Bool
    private let reconnect: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void

    /// Creates the page.
    ///
    /// - Parameters:
    ///   - instances: The instance store.
    ///   - catalog: The catalog, for the provider names and tiles, or `nil`.
    ///   - runtimes: The runtime store, for the Runtimes card and the names of the install
    ///     operations, or `nil`: the card is then not drawn.
    ///   - operations: The operation center.
    ///   - capacity: The disk reading.
    ///   - actions: Start, Stop, and Retry.
    ///   - isConnected: Whether the daemon is connected and every store refreshed: the disk
    ///     is read every 60 seconds only then.
    ///   - completedRounds: How many snapshot rounds completed: a change reads the disk again
    ///     and ends the pending clicks whose operation is gone.
    ///   - waiting: What a page without data shows while the daemon cannot be reached
    ///     ("Waiting for stacklyd"), or `nil` when the daemon is reachable or still loading.
    ///   - hasConnection: Whether a connection to the daemon is open: Retry then loads the
    ///     list again, else it reconnects.
    ///   - reconnect: Tries to connect now.
    ///   - openDiagnostics: Opens Diagnostics.
    public init(
        instances: InstanceStore,
        catalog: CatalogStore? = nil,
        runtimes: RuntimeStore? = nil,
        operations: OperationCenter,
        capacity: CapacityModel,
        actions: InstanceActionModel,
        isConnected: Bool,
        completedRounds: Int = 0,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void
    ) {
        self.instances = instances
        self.catalog = catalog
        self.runtimes = runtimes
        self.operations = operations
        self.capacity = capacity
        self.actions = actions
        self.isConnected = isConnected
        self.completedRounds = completedRounds
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
        let loaded = instances.loadedCount != nil
        let summaries = instances.instances
        let counts = loaded ? InstanceCounts(of: summaries) : nil
        StateView(
            state: Self.state(
                counts: counts, error: instances.store.error,
                unsupported: instances.store.unsupported, waiting: waiting,
                loadedAt: instances.store.loadedAt, isRefreshing: instances.store.isRefreshing,
                partial: partialNote(summaries)),
            retry: retry,
            openDiagnostics: openDiagnostics
        ) { counts in
            content(counts: counts, summaries: summaries)
        }
        .pageChrome(
            title: SidebarSection.overview.title, subtitle: counts.map(OverviewCopy.subtitle)
        )
        .toolbar {
            if let newInstance {
                ToolbarItem(placement: .primaryAction) {
                    ActionButton(
                        newInstance.stacklyAction(title: "New Instance"), showsReason: false
                    )
                    .help(CommandMap.helpText(for: .newInstance))
                }
            }
        }
        .task(id: isConnected) {
            await capacity.poll(isConnected: { isConnected })
        }
        .task(id: completedRounds) {
            await capacity.refresh()
            await actions.reconcile(
                activeOperations: Set(operations.active.map(\.id)),
                names: Dictionary(
                    summaries.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first }),
                context: context)
        }
    }

    // MARK: - Content

    private func content(counts: InstanceCounts, summaries: [InstanceSummary]) -> some View {
        let rows = InstanceRowModel.rows(
            of: summaries, catalog: catalog?.providers,
            failures: Set(instances.store.itemFailures.keys))
        let names = Dictionary(
            summaries.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first })
        let attention = attentionItems(summaries)
        return ScrollView {
            VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                SummaryCards(
                    counts: counts, runtimes: RuntimesCardState.of(runtimes),
                    show: { navigator?.show(.instances(filter: $0)) },
                    showRuntimes: { navigator?.show(.runtimes) })
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: Spacing.sectionGap) {
                        VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                            attentionSection(attention)
                            instancesSection(rows)
                        }
                        .frame(maxWidth: .infinity, alignment: .top)
                        operationsSection(names: names)
                            .frame(minWidth: 300, maxWidth: 360, alignment: .top)
                    }
                    .frame(minWidth: Self.twoColumnMinWidth)
                    VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                        attentionSection(attention)
                        instancesSection(rows)
                        operationsSection(names: names)
                    }
                }
            }
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s22)
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .background(StacklyColor.surfaceWindow)
    }

    private func attentionSection(_ items: [AttentionItem]?) -> some View {
        AttentionList(items: items, open: open)
    }

    private func open(_ destination: AttentionDestination) {
        switch destination {
        case .instance(let id): navigator?.openInstance(id)
        case .operations: navigator?.show(.operations)
        case .diagnostics: navigator?.show(.diagnostics)
        }
    }

    private func instancesSection(_ rows: [InstanceRowModel]) -> some View {
        OverviewInstanceList(
            rows: Array(rows.prefix(OverviewCopy.instanceLimit)), total: rows.count,
            action: { actions.stacklyAction(for: $0, context: context) },
            open: { navigator?.openInstance($0) },
            showAll: { navigator?.show(.instances(filter: .all)) },
            browseCatalog: { navigator?.show(.catalog) })
    }

    private func operationsSection(names: [UUID: String]) -> some View {
        OverviewOperations(
            content: operationsContent(),
            title: {
                OperationTitle.title(for: $0, instanceNames: names, runtimes: runtimeNames)
            },
            open: { navigator?.show(.operations) },
            openDiagnostics: { navigator?.show(.diagnostics) })
    }

    /// The runtime names the titles use, from the `RuntimeStore`.
    private var runtimeNames: OperationTitle.RuntimeNames { .init(store: runtimes) }

    // MARK: - Sources

    private func operationsContent() -> OverviewOperationsContent {
        Self.operationsContent(
            running: operations.active,
            loaded: operations.store.loadedAt != nil,
            failed: operations.store.error != nil,
            unsupported: operations.store.unsupported,
            retry: { Task { await operations.store.refresh() } })
    }

    /// The Operations section for the state of the center: only queued, running, and
    /// cancelling operations, newest first, at most five.
    static func operationsContent(
        running active: [OperationSnapshot], loaded: Bool, failed: Bool, unsupported: Bool,
        retry: @escaping @MainActor () -> Void
    ) -> OverviewOperationsContent {
        if unsupported { return .unsupported }
        let running = active.filter { $0.state != .needsRecovery }
        if loaded {
            return .loaded(
                rows: Array(running.prefix(OverviewCopy.operationLimit)), total: running.count)
        }
        return failed ? .failed(title: "Couldn’t Load Operations", retry: retry) : .loading
    }

    private func attentionItems(_ summaries: [InstanceSummary]) -> [AttentionItem]? {
        Self.attentionItems(
            summaries: summaries, instancesLoaded: instances.loadedCount != nil,
            operations: operations.active, operationsLoaded: operations.store.loadedAt != nil,
            capacity: capacity.state, runtimes: runtimeNames, now: fixtureNow ?? Date(),
            locale: locale, timeZone: timeZone)
    }

    /// The items, or `nil` while a source has not loaded: the instances, the operations
    /// (also when they failed), or the first disk reading.
    static func attentionItems(
        summaries: [InstanceSummary], instancesLoaded: Bool, operations: [OperationSnapshot],
        operationsLoaded: Bool, capacity: CapacityState, runtimes: OperationTitle.RuntimeNames,
        now: Date, locale: Locale, timeZone: TimeZone
    ) -> [AttentionItem]? {
        guard instancesLoaded, operationsLoaded, capacity.attempted else { return nil }
        return AttentionRules.items(
            instances: summaries, operations: operations, runtimes: runtimes, capacity: capacity,
            now: now, locale: locale, timeZone: timeZone)
    }

    // MARK: - State

    private func retry() {
        if hasConnection {
            Task { await instances.store.refresh() }
        } else {
            reconnect()
        }
    }

    private func partialNote(_ summaries: [InstanceSummary]) -> PartialNote? {
        InstancesPage.partial(
            failures: instances.store.itemFailures,
            names: Dictionary(
                summaries.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first }),
            retry: { id in Task { await instances.store.retry(id) } })
    }

    /// The state of the page: the fixed order of ``ViewState/resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)``.
    ///
    /// The page is never empty: the Instances section shows its own empty state.
    static func state(
        counts: InstanceCounts?, error: ErrorBody?, unsupported: Bool, waiting: FailureContent?,
        loadedAt: Date?, isRefreshing: Bool, partial: PartialNote?
    ) -> ViewState<InstanceCounts> {
        let storeFailure = error.map {
            FailureContent(title: "Couldn’t Load Overview", message: $0.message, code: $0.code)
        }
        return .resolve(
            unsupported: unsupported
                ? UnsupportedContent(
                    title: "Overview Unavailable",
                    reason: "This version of stacklyd doesn’t list instances. Update Stackly.")
                : nil,
            value: counts,
            error: counts == nil ? (waiting ?? storeFailure) : storeFailure,
            lastLoadedAt: loadedAt,
            isRefreshing: isRefreshing,
            emptyContent: { _ in nil },
            partial: partial)
    }
}

#if DEBUG
    /// Loads the fixture stores, then shows the page: a preview cannot await.
    private struct OverviewPagePreview: View {
        @State private var rig = PageRig(
            capacity: CapacityReport(
                checkedAt: FixtureClock.now, totalBytes: 994_662_584_320,
                availableBytes: 4_100_000_000, usedPercent: 99, level: .low))
        @State private var isLoaded = false

        var body: some View {
            Group {
                if isLoaded {
                    rig.overviewPage()
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

    #Preview("Light") {
        OverviewPagePreview()
            .frame(width: 1044, height: 748)
            .preferredColorScheme(.light)
    }

    #Preview("Dark") {
        OverviewPagePreview()
            .frame(width: 1044, height: 748)
            .preferredColorScheme(.dark)
    }

    #Preview("640 pt") {
        OverviewPagePreview()
            .frame(width: AppShell.contentMinWidth, height: 748)
            .preferredColorScheme(.light)
    }
#endif
