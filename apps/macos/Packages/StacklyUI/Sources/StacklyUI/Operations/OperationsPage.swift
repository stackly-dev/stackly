import StacklyClient
import SwiftUI

/// What the Operations page holds while it shows: its history, its actions, and the detail
/// of the inspector (spec 0035, *Halaman Operations*).
@MainActor
final class OperationsPageModel {
    /// The list: the history pages plus the center's changes.
    let history: OperationHistory
    /// Cancel, Resume, and Abandon.
    let actions: OperationActions
    /// The detail of the selected operation.
    let detail: OperationDetailModel

    /// The model of a page over `service` and `center`.
    init(service: OperationsService, center: OperationCenter) {
        let history = OperationHistory(service: service, center: center)
        self.history = history
        actions = OperationActions(service: service, history: history)
        detail = OperationDetailModel(history: history)
    }
}

/// The Operations page (spec 0035, *Halaman Operations*): the operations that run and the
/// history of `operation.list`, newest first and updated by events, each row with its note,
/// badge, Cancel, Retry, and progress, and the chosen one in the inspector with Cancel, Retry,
/// Resume, and Abandon. ``ShellNavigator/openOperation(_:)`` selects an operation and opens
/// the inspector, also one older than the loaded pages.
public struct OperationsPage: View {
    @SceneStorage("operations.selection") private var storedSelection = ""
    @SceneStorage("operations.inspector") private var showsInspector = true
    @State private var model: OperationsPageModel
    @State private var appliedRequest: UUID?
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.feedbackCenter) private var feedbackCenter

    private let operations: OperationCenter
    private let instances: InstanceStore?
    private let runtimes: RuntimeStore?
    private let instanceActions: InstanceActionModel?
    private let waiting: FailureContent?
    private let hasConnection: Bool
    private let reconnect: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void

    /// Creates the page.
    /// - Parameters:
    ///   - operations: The operation center: the operations that have not ended and the
    ///     events of every operation.
    ///   - instances: The instance store, for the titles and Retry, or `nil`.
    ///   - runtimes: The runtime store, for the titles of installs, or `nil`.
    ///   - actions: Start, Stop, Restart, and Reload of instances, for Retry, or `nil`.
    ///   - service: The history, the detail, and the mutations.
    ///   - waiting: What a page without data shows while the daemon cannot be reached, or
    ///     `nil` when the daemon is reachable or still loading.
    ///   - hasConnection: Whether a connection to the daemon is open: Retry then loads the
    ///     list again, else it reconnects.
    ///   - reconnect: Tries to connect now.
    ///   - openDiagnostics: Opens Diagnostics.
    ///   - selection: The operation ID a window with no saved choice starts on; empty selects
    ///     none. Previews and snapshots set it.
    public init(
        operations: OperationCenter,
        instances: InstanceStore? = nil,
        runtimes: RuntimeStore? = nil,
        actions: InstanceActionModel? = nil,
        service: OperationsService,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void,
        selection: String = ""
    ) {
        self.init(
            model: OperationsPageModel(service: service, center: operations),
            operations: operations, instances: instances, runtimes: runtimes, actions: actions,
            waiting: waiting, hasConnection: hasConnection, reconnect: reconnect,
            openDiagnostics: openDiagnostics, selection: selection)
    }

    /// Creates the page over a model that may already hold its history, for a snapshot.
    init(
        model: OperationsPageModel,
        operations: OperationCenter,
        instances: InstanceStore? = nil,
        runtimes: RuntimeStore? = nil,
        actions: InstanceActionModel? = nil,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void,
        selection: String = ""
    ) {
        _storedSelection = SceneStorage(wrappedValue: selection, "operations.selection")
        _model = State(initialValue: model)
        self.operations = operations
        self.instances = instances
        self.runtimes = runtimes
        self.instanceActions = actions
        self.waiting = waiting
        self.hasConnection = hasConnection
        self.reconnect = reconnect
        self.openDiagnostics = openDiagnostics
    }

    /// The page.
    public var body: some View {
        let history = model.history
        let rows = history.items
        let selectedID = selectionID
        StateView(state: state(rows: rows), retry: retry, openDiagnostics: openDiagnostics) {
            rows in
            content(rows)
        }
        .stacklyInspector(isPresented: $showsInspector) {
            inspector(selectedID: selectedID, rows: rows)
        }
        .pageChrome(title: SidebarSection.operations.title, subtitle: operations.subtitle)
        .task { await history.start() }
        .onDisappear { history.stop() }
        .onChange(of: navigator?.operationRequest, initial: true) { _, request in
            apply(request)
        }
        .task(id: DetailKey(selectedID, in: rows)) {
            if let selectedID {
                await model.detail.load(selectedID, revision: nil)
            }
        }
    }

    /// What makes the inspector read `operation.get` again: another selection, or a new state
    /// or stage of the selected operation, when its steps change. A progress event alone does
    /// not, so a download does not read it once per event; the summary on screen follows
    /// every event through the list.
    private struct DetailKey: Hashable {
        let id: UUID?
        let state: OperationState?
        let stage: String?

        init(_ id: UUID?, in rows: [OperationSnapshot]) {
            let operation = rows.first { $0.id == id }
            self.id = id
            state = operation?.state
            stage = operation?.stage
        }
    }

    // MARK: - List

    private func content(_ rows: [OperationSnapshot]) -> some View {
        let names = instanceNames
        let runtimeNames = OperationTitle.RuntimeNames(store: runtimes)
        return VStack(alignment: .leading, spacing: 0) {
            List(selection: selectionBinding) {
                ForEach(rows) { operation in
                    let title = OperationTitle.title(
                        for: operation, instanceNames: names, runtimes: runtimeNames)
                    OperationRow(
                        operation: operation, title: title,
                        actions: rowActions(for: operation, title: title, rows: rows)
                    )
                    .tag(operation.id)
                }
                if model.history.next != nil {
                    showMore
                        .selectionDisabled()
                }
            }
            Text(OperationsCopy.footer)
                .font(.footnote)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.vertical, Spacing.s12)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }

    @ViewBuilder private var showMore: some View {
        let history = model.history
        HStack(spacing: Spacing.s8) {
            Button("Show More") {
                Task { await history.showMore() }
            }
            .disabled(history.isLoadingMore)
            if history.isLoadingMore {
                DelayedProgress()
            } else if let error = history.moreError {
                Text("Couldn’t load more. \(DaemonText.sentence(error.message))")
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
        }
        .padding(.vertical, Spacing.s6)
    }

    // MARK: - Inspector

    @ViewBuilder
    private func inspector(selectedID: UUID?, rows: [OperationSnapshot]) -> some View {
        if let selectedID {
            let operation = rows.first { $0.id == selectedID }
            let phase =
                model.history.missing.contains(selectedID)
                ? .missing : model.detail.phase(for: selectedID)
            let shown = operation ?? Self.summary(of: phase)
            let title =
                shown.map {
                    OperationTitle.title(
                        for: $0, instanceNames: instanceNames,
                        runtimes: OperationTitle.RuntimeNames(store: runtimes))
                } ?? SidebarSection.operations.title
            OperationInspector(
                operation: operation, phase: phase, title: title,
                actions: shown.map { inspectorActions(for: $0, title: title, rows: rows) } ?? [],
                names: resourceNames,
                retry: { Task { await model.detail.load(selectedID, revision: nil) } })
        } else {
            InspectorPlaceholder(
                title: "No Selection", message: "Select an operation to see its details.")
        }
    }

    private static func summary(of phase: OperationDetailModel.Phase) -> OperationSnapshot? {
        if case .loaded(let detail) = phase { return detail.operation }
        return nil
    }

    // MARK: - Actions

    /// Cancel and Retry, for a row (AC-11).
    private func rowActions(
        for operation: OperationSnapshot, title: String, rows: [OperationSnapshot]
    ) -> [StacklyAction] {
        [cancelAction(operation, title: title), retryAction(operation, rows: rows)]
            .compactMap { $0 }
    }

    /// Cancel, Retry, Resume, and Abandon, for the inspector (AC-12).
    private func inspectorActions(
        for operation: OperationSnapshot, title: String, rows: [OperationSnapshot]
    ) -> [StacklyAction] {
        rowActions(for: operation, title: title, rows: rows)
            + [resumeAction(operation, title: title), abandonAction(operation, title: title)]
            .compactMap { $0 }
    }

    private func cancelAction(_ operation: OperationSnapshot, title: String) -> StacklyAction? {
        let button = OperationRules.cancel(for: operation)
        guard button != .hidden else { return nil }
        let actions = model.actions
        let report = self.report
        return StacklyAction(
            id: "operation.cancel", title: "Cancel", mutates: true,
            disabledReason: button == .disabled ? OperationRules.notCancellableReason : nil,
            isBusy: actions.inFlight[operation.id] == .cancel,
            perform: { Task { await actions.cancel(operation, title: title, report: report) } })
    }

    private func retryAction(_ operation: OperationSnapshot, rows: [OperationSnapshot])
        -> StacklyAction?
    {
        guard let instanceActions, let id = OperationRules.instanceID(of: operation),
            let instance = instances?.instances.first(where: { $0.id == id }),
            let verb = OperationRules.retry(
                for: operation, among: rows, instance: instance,
                local: instanceActions.localStatus(for: id))
        else { return nil }
        let context = InstanceActionContext.make(
            feedbackCenter: feedbackCenter, navigator: navigator)
        return StacklyAction(
            id: "operation.retry", title: "Retry", mutates: true,
            perform: { instanceActions.perform(verb, for: instance, context: context) })
    }

    private func resumeAction(_ operation: OperationSnapshot, title: String) -> StacklyAction? {
        guard OperationRules.canResume(operation) else { return nil }
        let actions = model.actions
        let report = self.report
        return StacklyAction(
            id: "operation.resume", title: "Resume", mutates: true,
            isBusy: actions.inFlight[operation.id] == .resume,
            perform: { Task { await actions.resume(operation, title: title, report: report) } })
    }

    private func abandonAction(_ operation: OperationSnapshot, title: String) -> StacklyAction? {
        guard OperationRules.canAbandon(operation) else { return nil }
        let actions = model.actions
        let report = self.report
        return StacklyAction(
            id: "operation.abandon", title: "Abandon…",
            role: .destructive(OperationActions.abandonConfirmation(title: title)),
            mutates: true, isBusy: actions.inFlight[operation.id] == .abandon,
            perform: { Task { await actions.abandon(operation, title: title, report: report) } })
    }

    private var report: @MainActor (Feedback) -> Void {
        let feedbackCenter = feedbackCenter
        return { feedbackCenter?.show($0) }
    }

    // MARK: - Names and selection

    private var instanceNames: [UUID: String] {
        Dictionary(
            (instances?.instances ?? []).map { ($0.id, $0.name) },
            uniquingKeysWith: { first, _ in first })
    }

    /// The instance and runtime names by ID, for the resource of the inspector.
    private var resourceNames: [UUID: String] {
        instanceNames.merging(
            OperationTitle.RuntimeNames(store: runtimes).installed,
            uniquingKeysWith: { first, _ in first })
    }

    private var selectionID: UUID? {
        storedSelection.isEmpty ? nil : UUID(uuidString: storedSelection)
    }

    private var selectionBinding: Binding<UUID?> {
        Binding {
            selectionID
        } set: { newValue in
            storedSelection = newValue?.uuidString.lowercased() ?? ""
        }
    }

    /// Selects the operation a request names and opens the inspector, once per request
    /// (AC-13).
    private func apply(_ request: OperationRequest?) {
        guard let request, appliedRequest != request.id else { return }
        appliedRequest = request.id
        storedSelection = request.operation.uuidString.lowercased()
        showsInspector = true
    }

    // MARK: - State

    private func retry() {
        if hasConnection {
            Task {
                if operations.store.error != nil { await operations.store.refresh() }
                await model.history.reload()
            }
        } else {
            reconnect()
        }
    }

    private func state(rows: [OperationSnapshot]) -> ViewState<[OperationSnapshot]> {
        let history = model.history
        return Self.state(
            rows: history.loadedAt == nil ? nil : rows, error: history.error,
            unsupported: operations.store.unsupported, waiting: waiting,
            loadedAt: history.loadedAt, isRefreshing: history.isRefreshing)
    }

    /// The state of the page (AC-11): "Operations Unavailable" for an older daemon, the rows
    /// once the first page loaded (a later failure keeps them with a stale note), "Couldn’t
    /// Load Operations" or "Waiting for stacklyd" before that, and "No Operations" for none.
    static func state(
        rows: [OperationSnapshot]?, error: ErrorBody?, unsupported: Bool,
        waiting: FailureContent?, loadedAt: Date?, isRefreshing: Bool
    ) -> ViewState<[OperationSnapshot]> {
        let failure = error.map {
            FailureContent(
                title: "Couldn’t Load Operations", message: DaemonText.sentence($0.message),
                code: $0.code)
        }
        return .resolve(
            unsupported: unsupported
                ? UnsupportedContent(
                    title: "Operations Unavailable", reason: OperationsCopy.unsupportedReason)
                : nil,
            value: rows,
            error: rows == nil ? (waiting ?? failure) : failure,
            lastLoadedAt: loadedAt,
            isRefreshing: isRefreshing,
            emptyContent: { rows in
                guard rows.isEmpty else { return nil }
                return EmptyContent.noData(
                    symbol: SidebarSection.operations.style.symbol, title: "No Operations",
                    message: OperationsCopy.emptyMessage)
            })
    }
}
