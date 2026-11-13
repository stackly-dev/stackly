import AppKit
import Foundation
import StacklyClient
import SwiftUI

/// The detail page of one environment (spec 0044, *Layar*, AC-18 to AC-22): the manifest
/// banner, the partially ready banner, the Services table with a menu per service, the
/// members the manifest no longer declares, Details, and Problems.
struct EnvironmentDetailPage: View {
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.feedbackCenter) private var feedbackCenter
    @Environment(\.environmentPlan) private var planPresenter

    let environments: EnvironmentStore
    let instances: InstanceStore?
    let catalog: CatalogStore?
    let actions: InstanceActionModel?
    let waiting: FailureContent?
    let hasConnection: Bool
    let reconnect: @MainActor () -> Void
    let openDiagnostics: @MainActor () -> Void
    let openIn: OpenInPorts
    let files: ManifestFilePorts
    let home: String

    @State private var model: EnvironmentDetailModel
    @State private var models: InstanceDetailModels
    @State private var columns = ServiceColumnSet.full
    @State private var tableSelection: String?
    @State private var openInNeeds: [SecretNeed]?

    init(
        id: UUID, environments: EnvironmentStore, instances: InstanceStore?,
        catalog: CatalogStore?, actions: InstanceActionModel?, waiting: FailureContent?,
        hasConnection: Bool, reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void, detail: InstanceDetailService?,
        openIn: OpenInPorts, files: ManifestFilePorts, home: String,
        model: EnvironmentDetailModel? = nil
    ) {
        self.environments = environments
        self.instances = instances
        self.catalog = catalog
        self.actions = actions
        self.waiting = waiting
        self.hasConnection = hasConnection
        self.reconnect = reconnect
        self.openDiagnostics = openDiagnostics
        self.openIn = openIn
        self.files = files
        self.home = home
        _model = State(
            initialValue: model ?? EnvironmentDetailModel(id: id, store: environments))
        _models = State(
            initialValue: InstanceDetailModels(service: detail ?? .disconnected, openIn: openIn))
    }

    private var context: InstanceActionContext {
        InstanceActionContext.make(feedbackCenter: feedbackCenter, navigator: navigator)
    }

    var body: some View {
        StateView(state: state, retry: retry, openDiagnostics: openDiagnostics) { detail in
            content(detail)
        }
        .toolbar { toolbar }
        .pageChrome(
            title: model.summary?.name ?? SidebarSection.environments.title,
            subtitle: model.summary.map { EnvironmentCopy.detailSubtitle($0, home: home) }
        )
        .task {
            // A model loaded before the page showed (a snapshot) is not read again.
            if model.detail == nil { await model.load(checkManifest: true) }
        }
        .onChange(of: environments.changes) {
            Task { await model.load(checkManifest: false) }
        }
        .onChange(of: environments.lastEnded) { _, ended in
            guard ended?.environment == model.id else { return }
            Task { await model.load(checkManifest: true) }
        }
        .onReceive(
            NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)
        ) { _ in
            Task { await model.load(checkManifest: true) }
        }
        .openInSecretSheet(
            needs: $openInNeeds, secrets: models.detailService.secrets, reload: {})
    }

    // MARK: - Toolbar

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            ActionButton(validateAction, prominence: .secondary, showsReason: false)
            if let plan = planAction {
                ActionButton(plan, prominence: .secondary, showsReason: false)
            }
            // Up… and Down of specs 0046 and 0047 go here, right of Plan….
            ActionMenu(moreSections, help: "More actions for this environment")
        }
    }

    private var validateAction: StacklyAction {
        StacklyAction(
            id: "environment.validate", title: "Validate", mutates: false,
            disabledReason: model.manifestLocation == nil
                ? "This environment has no manifest file." : nil,
            isBusy: model.isValidating,
            perform: { run { await model.validate() } })
    }

    /// Plan…: the plan sheet for the registered manifest, from the detail page with Done only.
    private var planAction: StacklyAction? {
        guard let planPresenter, let summary = model.summary else { return nil }
        return StacklyAction(
            id: "environment.plan", title: "Plan…", mutates: false,
            disabledReason: summary.manifestLocation == nil
                ? "This environment has no manifest file." : nil,
            perform: { planPresenter.review(environment: summary.id, name: summary.name) })
    }

    private var moreSections: [[StacklyAction]] {
        let model = model
        let reload: @MainActor () -> Void = { run { await model.reloadManifest() } }
        return EnvironmentFileActions.sections(
            location: model.manifestLocation, ports: files,
            reload: model.manifestLocation == nil ? nil : reload,
            remove: removeAction, isReloading: model.isImporting)
    }

    private var removeAction: StacklyAction? {
        guard let summary = model.summary else { return nil }
        return StacklyAction(
            id: "environment.remove", title: "Remove Environment…",
            role: .destructive(
                ActionConfirmation(
                    title: EnvironmentCopy.removeTitle(summary.name),
                    message: EnvironmentCopy.removeMessage,
                    actionTitle: EnvironmentCopy.removeAction)),
            mutates: true, isBusy: model.isRemoving,
            perform: {
                Task {
                    switch await model.remove() {
                    case .removed(let feedback):
                        feedbackCenter?.show(feedback)
                        navigator?.show(.environments)
                    case .kept(let feedback):
                        if let feedback { feedbackCenter?.show(feedback) }
                    }
                }
            })
    }

    /// Runs `work` and shows the feedback it returns.
    private func run(_ work: @escaping @MainActor () async -> Feedback?) {
        Task {
            if let feedback = await work() { feedbackCenter?.show(feedback) }
        }
    }

    // MARK: - Content

    private func content(_ detail: EnvironmentDetail) -> some View {
        let rows = ServiceRowModel.rows(of: detail, instances: instances?.store.items ?? [:])
        return ScrollView {
            VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                if let banner = manifestBanner(detail) {
                    banner
                }
                if detail.summary.readyCount > 0,
                    detail.summary.readyCount < detail.summary.serviceCount
                {
                    StateBanner(
                        tone: .caution, title: EnvironmentCopy.readyText(detail.summary),
                        items: ServiceRowModel.notReadyItems(
                            of: detail, open: { navigator?.openInstance($0) }))
                }
                servicesSection(rows)
                if let detached = detail.detached, detached.isEmpty == false {
                    notInManifest(detached)
                }
                if let findings = model.findings {
                    problemsSection(findings)
                }
                detailsSection(detail.summary)
            }
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .onGeometryChange(for: ServiceColumnSet.self) {
            ServiceColumnSet(width: $0.size.width - 2 * Spacing.pageHorizontal)
        } action: {
            columns = $0
        }
    }

    // MARK: - Banners

    private func manifestBanner(_ detail: EnvironmentDetail) -> StateBanner? {
        guard let state = model.manifestState else { return nil }
        let summary = detail.summary
        switch state {
        case .inSync, .unrecognized:
            return nil
        case .changed:
            var actions: [StateAction] = []
            if let planPresenter {
                actions.append(
                    StateAction(
                        title: "Review Plan", mutates: false,
                        perform: {
                            planPresenter.review(environment: summary.id, name: summary.name)
                        }))
            }
            actions.append(
                StateAction(
                    title: "Reload", mutates: true,
                    perform: { run { await model.reloadManifest() } }))
            return StateBanner(
                tone: .caution, title: EnvironmentCopy.changedTitle, actions: actions)
        case .missing, .unreadable:
            var actions: [StateAction] = []
            if let planPresenter {
                actions.append(
                    StateAction(
                        title: "Locate…", mutates: true,
                        perform: {
                            Task {
                                guard let path = await planPresenter.chooseFile() else { return }
                                run { await model.locate(path) }
                            }
                        }))
            }
            return StateBanner(
                tone: .critical, title: EnvironmentCopy.unreadableTitle, actions: actions)
        case .invalid:
            return StateBanner(
                tone: .critical, title: EnvironmentCopy.invalidTitle(model.problemCount),
                actions: [
                    StateAction(
                        title: "Show Problems", mutates: false,
                        perform: { run { await model.showProblems() } })
                ])
        case .nameChanged:
            var actions: [StateAction] = []
            if let planPresenter, let path = summary.manifestLocation {
                actions.append(
                    StateAction(
                        title: "Import as New", mutates: true,
                        perform: { planPresenter.importFile(path) }))
            }
            return StateBanner(
                tone: .caution, title: EnvironmentCopy.nameChangedTitle(model.declaredName),
                actions: actions)
        }
    }

    // MARK: - Services

    private func servicesSection(_ rows: [ServiceRowModel]) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            Text("Services")
                .font(.headline)
            servicesTable(rows)
                .frame(height: Self.tableHeight(rows: rows.count))
        }
    }

    /// The height that shows every row without scrolling: at most 32 services.
    static func tableHeight(rows: Int) -> CGFloat {
        CGFloat(max(rows, 1)) * 30 + 32
    }

    private func servicesTable(_ rows: [ServiceRowModel]) -> some View {
        Table(rows, selection: $tableSelection) {
            TableColumn("Service") { row in
                Text(row.alias).lineLimit(1).help(row.alias)
            }
            .width(min: 80, ideal: 120)
            if columns.showsSelector {
                TableColumn("Selector") { row in
                    Text(row.selector).lineLimit(1).truncationMode(.middle).help(row.selector)
                }
                .width(min: 110, ideal: 170)
            }
            TableColumn("Instance") { row in
                Text(row.instanceText)
                    .lineLimit(1)
                    .foregroundStyle(
                        row.instanceID == nil
                            ? AnyShapeStyle(StacklyColor.textSecondary) : AnyShapeStyle(.primary)
                    )
                    .help(row.instanceText)
            }
            .width(min: 100, ideal: 150)
            if columns.showsVersion {
                TableColumn("Version") { row in
                    Text(row.version).lineLimit(1)
                }
                .width(min: 60, ideal: 80, max: 110)
            }
            TableColumn("Ownership") { row in
                Text(row.ownership).lineLimit(1)
            }
            .width(min: 80, ideal: 100, max: 120)
            TableColumn("Status") { row in
                StatusLabel(row.appearance, context: .row)
                    .help(
                        row.appearance.detail.map { "\(row.appearance.label): \($0.text)" }
                            ?? row.appearance.label)
            }
            .width(min: 100, ideal: 160, max: 240)
            TableColumn("") { row in
                ActionMenu(menu(for: row), title: "Actions", help: "Actions for \(row.alias)")
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                    .task(id: row.instanceID) { await loadDetail(of: row) }
            }
            .width(min: 44, ideal: 60, max: 80)
        }
        .id(columns)
        .scrollDisabled(true)
        .contextMenu(forSelectionType: String.self) { aliases in
            if let alias = aliases.first, let row = rows.first(where: { $0.id == alias }) {
                ActionMenuItems(menu(for: row))
            }
        } primaryAction: { aliases in
            guard let alias = aliases.first, let row = rows.first(where: { $0.id == alias }),
                let id = row.instanceID
            else { return }
            navigator?.openInstance(id)
        }
    }

    /// Loads the detail of a service's instance once, so its Open In actions and connection
    /// string are in the menu when it opens.
    private func loadDetail(of row: ServiceRowModel) async {
        guard let id = row.instanceID else { return }
        let detailModel = models.model(for: id)
        if detailModel.detail == nil {
            await detailModel.reload()
        }
    }

    /// The menu of a service (spec 0044, AC-19): Open In, Copy Connection String, Show
    /// Instance and Show Logs, then Start, Stop, or Retry. A service whose instance does not
    /// exist has only Show Plan….
    func menu(for row: ServiceRowModel) -> [[StacklyAction]] {
        guard let id = row.instanceID else {
            guard let planPresenter, let summary = model.summary else { return [] }
            return [
                [
                    StacklyAction(
                        id: "environment.showPlan", title: "Show Plan…", mutates: false,
                        perform: {
                            planPresenter.review(environment: summary.id, name: summary.name)
                        })
                ]
            ]
        }
        var sections: [[StacklyAction]] = []
        let detail = models.model(for: id).detail
        let content = models.openIn.content(
            summary: row.summary, detail: detail, report: { feedbackCenter?.show($0) },
            enterSecret: { openInNeeds = [$0] })
        if case .sections(let openInSections) = content {
            sections.append(contentsOf: openInSections)
        }
        sections.append([
            ConnectionCopier.action(
                detail: detail, secrets: models.detailService.secrets,
                report: { feedbackCenter?.show($0) })
        ])
        sections.append([
            StacklyAction(
                id: "environment.showInstance", title: "Show Instance", mutates: false,
                perform: { navigator?.openInstance(id) }),
            StacklyAction(
                id: "environment.showLogs", title: "Show Logs", mutates: false,
                perform: { navigator?.openInstance(id, tab: .logs) }),
        ])
        if let summary = row.summary, let actions {
            sections.append([actions.stacklyAction(for: summary, context: context)])
        }
        return sections
    }

    private func notInManifest(_ detached: [DetachedMember]) -> some View {
        CardSection("Not in Manifest") {
            ForEach(detached, id: \.alias) { member in
                LabeledContent(member.alias) {
                    Text(
                        "\(member.instance.name) · \(EnvironmentCopy.ownershipText(member.ownership))"
                    )
                }
            }
        }
        .labeledContentStyle(.stackly)
    }

    // MARK: - Details and Problems

    private func detailsSection(_ summary: EnvironmentSummary) -> some View {
        CardSection("Details") {
            LabeledContent("Manifest") {
                if let location = summary.manifestLocation {
                    HStack(spacing: Spacing.s6) {
                        Text(EnvironmentCopy.file(location, home: home))
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .textSelection(.enabled)
                        CopyButton(location, what: "Path", style: .icon)
                    }
                } else {
                    Text("—")
                }
            }
            LabeledContent("Schema") { Text(EnvironmentCopy.schema) }
            // The Lock and Local override rows of spec 0045 and the Lease row of spec 0047
            // go below.
        }
        .labeledContentStyle(.stackly)
    }

    private func problemsSection(_ findings: [EnvFinding]) -> some View {
        CardSection(
            "Problems",
            footer: model.omittedFindings > 0
                ? "\(model.omittedFindings) more problems are not shown." : ""
        ) {
            CheckList(
                findings.enumerated().map { index, finding in
                    CheckItem(
                        id: "\(index)-\(finding.code)",
                        status: finding.severity == .error ? .failed : .warning,
                        title: finding.message, detail: finding.location)
                }, layout: .single)
        }
    }

    // MARK: - State

    private func retry() {
        if hasConnection {
            Task { await model.load(checkManifest: true) }
        } else {
            reconnect()
        }
    }

    private var state: ViewState<EnvironmentDetail> {
        Self.state(
            detail: model.detail, error: model.error, waiting: waiting,
            isRefreshing: model.isLoading)
    }

    /// The state of the detail page: an unknown environment says so; "Waiting for stacklyd"
    /// counts only while there is no detail.
    static func state(
        detail: EnvironmentDetail?, error: ErrorBody?, waiting: FailureContent?,
        isRefreshing: Bool
    ) -> ViewState<EnvironmentDetail> {
        let failure = error.map { body in
            body.code == "env.not_found"
                ? FailureContent(
                    title: EnvironmentCopy.goneTitle, message: EnvironmentCopy.goneMessage,
                    code: body.code)
                : FailureContent(
                    title: EnvironmentCopy.detailFailedTitle, message: body.message,
                    code: body.code)
        }
        return .resolve(
            value: detail, error: detail == nil ? (waiting ?? failure) : failure,
            lastLoadedAt: nil, isRefreshing: isRefreshing, emptyContent: { _ in nil })
    }
}
