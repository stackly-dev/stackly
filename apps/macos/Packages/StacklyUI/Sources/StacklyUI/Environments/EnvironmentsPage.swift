import Foundation
import StacklyClient
import SwiftUI

/// The Environments section (spec 0044, *Layar*, AC-18): the list of environments, or the
/// detail page of the one the sidebar selects.
public struct EnvironmentsPage: View {
    @Environment(\.shellNavigator) private var navigator

    private let environments: EnvironmentStore
    private let instances: InstanceStore?
    private let catalog: CatalogStore?
    private let actions: InstanceActionModel?
    private let operations: OperationCenter?
    private let selected: UUID?
    private let waiting: FailureContent?
    private let hasConnection: Bool
    private let reconnect: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void
    private let detail: InstanceDetailService?
    private let openIn: OpenInPorts
    private let files: ManifestFilePorts
    private let home: String
    @State private var presenter: EnvironmentPlanPresenter
    @State private var downModel: EnvironmentDownModel
    /// A detail model loaded before the page shows, for previews and snapshots.
    private let detailModel: EnvironmentDetailModel?
    @Environment(\.feedbackCenter) private var feedbackCenter

    /// Creates the section.
    /// - Parameters:
    ///   - environments: The environment store.
    ///   - instances: The instance store, for the status, Open In, and actions of a service's
    ///     instance, or `nil`.
    ///   - catalog: The catalog, for the names of providers, or `nil`.
    ///   - actions: Start, Stop, and Retry of instances, or `nil` for none.
    ///   - operations: The operation center, for the progress and Cancel of an Up, or `nil`.
    ///   - selected: The environment whose detail page shows, or `nil` for the list.
    ///   - waiting: What a page without data shows while the daemon cannot be reached, or
    ///     `nil`.
    ///   - hasConnection: Whether a connection is open: Retry then loads again, else it
    ///     reconnects.
    ///   - reconnect: Tries to connect now.
    ///   - openDiagnostics: Opens Diagnostics.
    ///   - detail: The instance reads behind Open In and Copy Connection String, or `nil`.
    ///   - openIn: What Open In asks of the system.
    ///   - home: The home folder, written as `~` in paths.
    public init(
        environments: EnvironmentStore,
        instances: InstanceStore? = nil,
        catalog: CatalogStore? = nil,
        actions: InstanceActionModel? = nil,
        operations: OperationCenter? = nil,
        selected: UUID?,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void,
        detail: InstanceDetailService? = nil,
        openIn: OpenInPorts = .system,
        home: String = NSHomeDirectory()
    ) {
        self.init(
            environments: environments, instances: instances, catalog: catalog,
            actions: actions, operations: operations, selected: selected, waiting: waiting,
            hasConnection: hasConnection, reconnect: reconnect,
            openDiagnostics: openDiagnostics, detail: detail, openIn: openIn, files: .system,
            picker: OpenPanelFilePicker(), home: home)
    }

    init(
        environments: EnvironmentStore,
        instances: InstanceStore?,
        catalog: CatalogStore?,
        actions: InstanceActionModel?,
        operations: OperationCenter? = nil,
        selected: UUID?,
        waiting: FailureContent?,
        hasConnection: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void,
        detail: InstanceDetailService?,
        openIn: OpenInPorts,
        files: ManifestFilePorts,
        picker: any FilePicking,
        home: String,
        presenter: EnvironmentPlanPresenter? = nil,
        detailModel: EnvironmentDetailModel? = nil,
        downModel: EnvironmentDownModel? = nil
    ) {
        self.detailModel = detailModel
        _presenter = State(
            initialValue: presenter ?? EnvironmentPlanPresenter(store: environments, picker: picker)
        )
        _downModel = State(initialValue: downModel ?? EnvironmentDownModel(store: environments))
        self.files = files
        self.environments = environments
        self.instances = instances
        self.catalog = catalog
        self.actions = actions
        self.operations = operations
        self.selected = selected
        self.waiting = waiting
        self.hasConnection = hasConnection
        self.reconnect = reconnect
        self.openDiagnostics = openDiagnostics
        self.detail = detail
        self.openIn = openIn
        self.home = home
    }

    /// The section.
    public var body: some View {
        page
            .environment(\.environmentPlan, presenter)
            .environment(\.environmentDown, downModel)
            .stacklyConfirmation(item: $downModel.confirmation)
            .onChange(of: environments.lastEnded) { _, ended in
                Task { await downModel.ended(ended) }
            }
            .sheet(isPresented: sheetShown) {
                EnvironmentPlanSheet(
                    presenter: presenter, catalog: catalog, secrets: detail?.secrets,
                    instances: instances, home: home,
                    reviewProvider: { provider in
                        presenter.close()
                        navigator?.show(.provider(provider))
                    })
            }
            .onAppear {
                connectImport()
                connectDown()
            }
    }

    private var sheetShown: Binding<Bool> {
        Binding {
            presenter.isPresented
        } set: { shown in
            if shown == false { presenter.close() }
        }
    }

    /// After an import, the sidebar selects the environment and the window says so.
    private func connectImport() {
        let navigator = navigator
        let feedbackCenter = feedbackCenter
        presenter.imported = { detail in
            navigator?.openEnvironment(detail.summary.id)
            feedbackCenter?.show(.success(EnvironmentCopy.imported(detail.summary.name)))
        }
    }

    /// The result of a Down shows in the window, and Show Operation opens Activity.
    private func connectDown() {
        let navigator = navigator
        let feedbackCenter = feedbackCenter
        downModel.report = { feedbackCenter?.show($0) }
        downModel.openOperation = { navigator?.openOperation($0) }
    }

    @ViewBuilder private var page: some View {
        if let selected {
            EnvironmentDetailPage(
                id: selected, environments: environments, instances: instances,
                catalog: catalog, actions: actions, operations: operations, waiting: waiting,
                hasConnection: hasConnection, reconnect: reconnect,
                openDiagnostics: openDiagnostics, detail: detail, openIn: openIn, files: files,
                home: home, model: detailModel?.id == selected ? detailModel : nil
            )
            .id(selected)
        } else {
            EnvironmentListPage(
                environments: environments, waiting: waiting, hasConnection: hasConnection,
                reconnect: reconnect, openDiagnostics: openDiagnostics, home: home)
        }
    }
}

/// The Environments list: one row per environment with its status and "{n} of {m} ready ·
/// {file}"; a click opens it.
struct EnvironmentListPage: View {
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.environmentPlan) private var importer

    let environments: EnvironmentStore
    let waiting: FailureContent?
    let hasConnection: Bool
    let reconnect: @MainActor () -> Void
    let openDiagnostics: @MainActor () -> Void
    let home: String

    var body: some View {
        StateView(state: state, retry: retry, openDiagnostics: openDiagnostics) { list in
            rows(list)
        }
        .toolbar { importItem }
        .pageChrome(title: SidebarSection.environments.title, subtitle: environments.subtitle)
    }

    @ToolbarContentBuilder private var importItem: some ToolbarContent {
        if let action = importer?.stacklyAction() {
            ToolbarItem(placement: .primaryAction) {
                ActionButton(action, prominence: .secondary, showsReason: false)
                    .mutationGated()
            }
        }
    }

    private func rows(_ list: [EnvironmentSummary]) -> some View {
        List(selection: selection) {
            ForEach(list) { environment in
                EnvironmentRow(environment: environment, home: home)
                    .tag(environment.id)
            }
        }
        .listStyle(.inset)
    }

    /// A click on a row opens its environment; the list itself never keeps a selection.
    private var selection: Binding<UUID?> {
        Binding {
            nil
        } set: { id in
            if let id { navigator?.openEnvironment(id) }
        }
    }

    private func retry() {
        if hasConnection {
            Task { await environments.store.refresh() }
        } else {
            reconnect()
        }
    }

    private var state: ViewState<[EnvironmentSummary]> {
        Self.state(
            list: environments.environments, error: environments.store.error,
            unsupported: environments.store.unsupported, waiting: waiting,
            loadedAt: environments.store.loadedAt,
            isRefreshing: environments.store.isRefreshing,
            importAction: importer?.stateAction())
    }

    /// The state of the list: the fixed order of ``ViewState``. "Waiting for stacklyd" counts
    /// only while there is no data.
    static func state(
        list: [EnvironmentSummary]?, error: ErrorBody?, unsupported: Bool,
        waiting: FailureContent?, loadedAt: Date?, isRefreshing: Bool,
        importAction: StateAction?
    ) -> ViewState<[EnvironmentSummary]> {
        let failure = error.map {
            FailureContent(
                title: EnvironmentCopy.loadFailedTitle, message: $0.message, code: $0.code)
        }
        return .resolve(
            unsupported: unsupported
                ? UnsupportedContent(
                    title: EnvironmentCopy.unsupportedTitle,
                    reason: EnvironmentCopy.unsupportedReason)
                : nil,
            value: list,
            error: list == nil ? (waiting ?? failure) : failure,
            lastLoadedAt: loadedAt,
            isRefreshing: isRefreshing,
            emptyContent: { all in
                guard all.isEmpty else { return nil }
                return .noData(
                    symbol: SidebarSection.environments.style.symbol,
                    title: EnvironmentCopy.emptyTitle, message: EnvironmentCopy.emptyMessage,
                    primaryAction: importAction)
            })
    }
}

/// One environment of the list: its name, its status, and "{n} of {m} ready · {file}".
struct EnvironmentRow: View {
    let environment: EnvironmentSummary
    let home: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(environment.name)
                    .font(.body.weight(.medium))
                    .lineLimit(1)
                Text(EnvironmentCopy.rowSubtitle(environment, home: home))
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: Spacing.s8)
            StatusLabel(StatusAppearance.environment(environment), context: .row)
        }
        .padding(.vertical, Spacing.s4)
        .contentShape(Rectangle())
    }
}
