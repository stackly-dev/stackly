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
    @Environment(\.environmentDown) private var downModel
    @Environment(\.actionHost) private var actionHost
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    let environments: EnvironmentStore
    let instances: InstanceStore?
    let catalog: CatalogStore?
    let actions: InstanceActionModel?
    /// The operation center, for the progress and Cancel of an Up (spec 0046), or `nil`.
    let operations: OperationCenter?
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
    @State private var operationActions: OperationActions

    init(
        id: UUID, environments: EnvironmentStore, instances: InstanceStore?,
        catalog: CatalogStore?, actions: InstanceActionModel?, operations: OperationCenter? = nil,
        waiting: FailureContent?,
        hasConnection: Bool, reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void, detail: InstanceDetailService?,
        openIn: OpenInPorts, files: ManifestFilePorts, home: String,
        model: EnvironmentDetailModel? = nil
    ) {
        self.environments = environments
        self.instances = instances
        self.catalog = catalog
        self.actions = actions
        self.operations = operations
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
        _operationActions = State(
            initialValue: OperationActions(
                send: { [environments] call, key in await environments.send(call, key) },
                apply: { _ in }, reread: { _ in }))
    }

    private var context: InstanceActionContext {
        InstanceActionContext.make(
            feedbackCenter: feedbackCenter, navigator: navigator, host: actionHost)
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
            needs: $openInNeeds, secrets: models.detailService.secrets,
            reload: { Task { await model.load(checkManifest: false) } })
    }

    // MARK: - Toolbar

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            ActionButton(validateAction, prominence: .secondary, showsReason: false)
            if let down = downAction {
                ActionButton(down, prominence: .secondary, showsReason: false)
                    .mutationGated()
            }
            if let up = upAction {
                ActionButton(up, prominence: .primary, showsReason: false)
                    .mutationGated()
            }
            ActionMenu(moreSections, help: "More actions for this environment")
        }
    }

    private var validateAction: StacklyAction {
        let model = model
        return Self.validateAction(
            location: model.manifestLocation, isValidating: model.isValidating,
            perform: { run { await model.validate() } })
    }

    /// Validate of the toolbar, off without a manifest file.
    static func validateAction(
        location: String?, isValidating: Bool, perform: @escaping @MainActor () -> Void
    ) -> StacklyAction {
        StacklyAction(
            id: "environment.validate", title: "Validate", mutates: false,
            disabledReason: location == nil ? EnvironmentCopy.noManifest : nil,
            isBusy: isValidating, perform: perform)
    }

    /// Down: the preview and the confirmation of spec 0047 (AC-20), off with its reason while
    /// a Down of this environment runs. It stays on while an Up runs, which Down cancels.
    private var downAction: StacklyAction? {
        guard let downModel, let summary = model.summary else { return nil }
        let id = summary.id
        return Self.downAction(
            summary: summary, isBusy: downModel.isBusy(id),
            perform: { Task { await downModel.begin(environment: id) } })
    }

    /// Down of the toolbar: off with "Already bringing down" while an `environment_down` of
    /// the environment has not ended. Whether anything needs to stop is the preview's answer.
    static func downAction(
        summary: EnvironmentSummary, isBusy: Bool, perform: @escaping @MainActor () -> Void
    ) -> StacklyAction {
        StacklyAction(
            id: "environment.down", title: EnvironmentCopy.down, mutates: true,
            disabledReason: summary.activeOperation?.kind == .environmentDown
                ? EnvironmentCopy.alreadyBringingDown : nil,
            isBusy: isBusy, perform: perform)
    }

    /// Up…: the plan sheet for the registered manifest with Cancel and Bring Up (spec 0046,
    /// AC-19), off with its reason while an Up or a Down runs or while the manifest can't be
    /// used.
    private var upAction: StacklyAction? {
        guard let planPresenter, let summary = model.summary else { return nil }
        return Self.upAction(
            summary: summary, manifestState: model.manifestState,
            problemCount: model.problemCount, declaredName: model.declaredName,
            perform: { planPresenter.review(environment: summary.id, name: summary.name) })
    }

    /// Up… of the toolbar, off with the reason of ``upDisabledReason(summary:manifestState:problemCount:declaredName:)``.
    static func upAction(
        summary: EnvironmentSummary, manifestState: ManifestState?, problemCount: UInt64,
        declaredName: String?, perform: @escaping @MainActor () -> Void
    ) -> StacklyAction {
        StacklyAction(
            id: "environment.up", title: EnvironmentCopy.up, mutates: true,
            disabledReason: upDisabledReason(
                summary: summary, manifestState: manifestState, problemCount: problemCount,
                declaredName: declaredName),
            perform: perform)
    }

    /// Why Up… is off, or `nil` (spec 0046, *Layar*): an operation of this environment that has
    /// not ended, no manifest file, or a manifest that is `missing`, `unreadable`, `invalid`,
    /// or `name_changed`, with the words of its banner.
    static func upDisabledReason(
        summary: EnvironmentSummary, manifestState: ManifestState?, problemCount: UInt64,
        declaredName: String?
    ) -> String? {
        if let active = summary.activeOperation {
            return active.kind == .environmentDown
                ? EnvironmentCopy.bringingDown : EnvironmentCopy.alreadyBringingUp
        }
        guard summary.manifestLocation != nil else { return EnvironmentCopy.noManifest }
        switch manifestState {
        case .missing, .unreadable: return EnvironmentCopy.unreadableTitle
        case .invalid: return EnvironmentCopy.invalidTitle(problemCount)
        case .nameChanged: return EnvironmentCopy.nameChangedTitle(declaredName)
        case .inSync, .changed, .unrecognized, nil: return nil
        }
    }

    private var moreSections: [[StacklyAction]] {
        let model = model
        let reload: @MainActor () -> Void = { run { await model.reloadManifest() } }
        let lock = EnvironmentFileActions.lockSection(
            location: model.manifestLocation, lockFile: model.lockFile,
            isLocking: model.isLocking, isExporting: model.isExporting,
            lock: { run { await model.lockVersions(refresh: .none) } },
            update: { run { await model.lockVersions(refresh: .all) } },
            copy: { run { await model.copyVariables() } })
        return EnvironmentFileActions.sections(
            location: model.manifestLocation, ports: files,
            reload: model.manifestLocation == nil ? nil : reload,
            lock: lock, remove: removeAction, isReloading: model.isImporting)
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
                    case .kept(var feedback):
                        if feedback?.code == "env.lease_active", let downModel {
                            let id = summary.id
                            feedback?.action = StacklyAction(
                                id: "environment.remove.down",
                                title: EnvironmentCopy.bringDownAgain, mutates: true,
                                perform: { Task { await downModel.begin(environment: id) } })
                        }
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
                if let blockers = model.lockBlockers {
                    StateBanner(
                        tone: .critical, title: EnvironmentCopy.lockBlocked(blockers.count),
                        items: EnvironmentPlanContent.items(blockers),
                        actions: [
                            StateAction(
                                title: "Dismiss", mutates: false,
                                perform: { model.closeLockBlockers() })
                        ])
                }
                if let banner = upBanner(detail) {
                    banner
                }
                servicesSection(rows)
                if let detached = detail.detached, detached.isEmpty == false {
                    notInManifest(detached)
                }
                if let findings = model.findings {
                    problemsSection(findings)
                }
                detailsSection(detail)
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

    // MARK: - Up (spec 0046)

    /// The banner of the environment's state after spec 0046: "Bringing up… {done} of {total}"
    /// while an Up runs; the partially ready banner with the reason and action of each service
    /// that is not ready; or "Not ready" when no service is ready after an Up that failed or
    /// was interrupted. While a Down runs, "Bringing down… {done} of {total}" (spec 0047).
    private func upBanner(_ detail: EnvironmentDetail) -> StateBanner? {
        let summary = detail.summary
        if let active = summary.activeOperation {
            switch active.kind {
            case .environmentUp:
                return progressBanner(active.id, kind: .environmentUp, lastUp: detail.lastUp)
            case .environmentDown:
                return progressBanner(active.id, kind: .environmentDown, lastUp: nil)
            default:
                return nil
            }
        }
        let interrupted = detail.lastUp?.state == .interrupted
        let message = interrupted ? EnvironmentCopy.interrupted : nil
        let navigator = navigator
        let items = ServiceRowModel.notReadyItems(
            of: detail,
            actions: ServiceRowModel.BannerActions(
                open: { navigator?.openInstance($0) },
                openOperation: { navigator?.openOperation($0) },
                setSecret: { openInNeeds = $0 }))
        if summary.readyCount > 0, summary.readyCount < summary.serviceCount {
            return StateBanner(
                tone: .caution, title: EnvironmentCopy.readyText(summary), message: message,
                items: items)
        }
        if summary.readyCount == 0, summary.serviceCount > 0,
            detail.lastUp?.state == .failed || interrupted
        {
            return StateBanner(
                tone: .caution, title: EnvironmentCopy.notReadyTitle, message: message,
                items: items)
        }
        return nil
    }

    /// "Bringing up… {done} of {total}" or "Bringing down… {done} of {total}" with Cancel,
    /// only while the operation allows it, and Show in Activity.
    private func progressBanner(_ id: UUID, kind: OperationKind, lastUp: EnvUpReport?)
        -> StateBanner
    {
        let operation = operations?.store.items[id]
        let (done, total) = Self.progress(operation: operation, lastUp: lastUp)
        let titled: (UInt64?, UInt64?) -> String =
            kind == .environmentDown
            ? { EnvironmentCopy.bringingDown(done: $0, total: $1) }
            : { EnvironmentCopy.bringingUp(done: $0, total: $1) }
        var actions: [StateAction] = []
        if let operation, OperationCancel.availability(of: operation) == .available {
            let operationActions = operationActions
            let title = titled(nil, nil)
            let report: @MainActor (Feedback) -> Void = { feedbackCenter?.show($0) }
            actions.append(
                StateAction(
                    title: "Cancel", mutates: true,
                    perform: {
                        Task {
                            await operationActions.cancel(operation, title: title, report: report)
                        }
                    }))
        }
        let navigator = navigator
        actions.append(
            StateAction(
                title: EnvironmentCopy.showInActivity, mutates: false,
                perform: { navigator?.openOperation(id) }))
        return StateBanner(tone: .informative, title: titled(done, total), actions: actions)
    }

    /// "{done} of {total}" from `OperationSummary.progress`, or from the services of the Up
    /// report that ended when the operation center does not hold the operation.
    static func progress(operation: OperationSummary?, lastUp: EnvUpReport?)
        -> (done: UInt64?, total: UInt64?)
    {
        if case .counted(let counted) = operation?.progress {
            return (counted.done, counted.total)
        }
        guard let services = lastUp?.services, services.isEmpty == false else { return (nil, nil) }
        let ended = services.filter { service in
            switch service.phase {
            case .ready, .failed, .needsSecret, .skipped, .cancelled: true
            case .pending, .installing, .creating, .waiting, .starting, .unrecognized: false
            }
        }
        return (UInt64(ended.count), UInt64(services.count))
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
            .width(min: 70, ideal: columns.isCompact ? 90 : 120)
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
            .width(min: 80, ideal: columns.isCompact ? 110 : 150)
            if columns.showsVersion {
                TableColumn("Version") { row in
                    Text(row.version).lineLimit(1)
                }
                .width(min: 60, ideal: 80, max: 110)
            }
            TableColumn("Ownership") { row in
                Text(row.ownership).lineLimit(1)
            }
            .width(min: 70, ideal: columns.isCompact ? 84 : 100, max: 120)
            TableColumn("Status") { row in
                StatusLabel(row.appearance, context: .row)
                    .help(
                        row.appearance.detail.map { "\(row.appearance.label): \($0.text)" }
                            ?? row.appearance.label)
            }
            .width(min: 90, ideal: columns.isCompact ? 120 : 160, max: 240)
            TableColumn("") { row in
                ActionMenu(menu(for: row), title: "Actions", help: "Actions for \(row.alias)")
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                    .task(id: row.instanceID) { await loadDetail(of: row) }
            }
            .width(min: 44, ideal: 52, max: 80)
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

    /// The menu of a service row, from what the page holds.
    func menu(for row: ServiceRowModel) -> [[StacklyAction]] {
        let summary = model.summary
        let planPresenter = planPresenter
        var showPlan: (@MainActor () -> Void)?
        if let planPresenter, let summary {
            showPlan = { planPresenter.review(environment: summary.id, name: summary.name) }
        }
        let detail = row.instanceID.flatMap { models.model(for: $0).detail }
        let navigator = navigator
        return ServiceMenu.sections(
            for: row,
            openIn: row.instanceID == nil
                ? .hidden
                : models.openIn.content(
                    summary: row.summary, detail: detail, report: { feedbackCenter?.show($0) },
                    enterSecret: { openInNeeds = [$0] }),
            copy: row.instanceID == nil
                ? nil
                : ConnectionCopier.action(
                    detail: detail, secrets: models.detailService.secrets,
                    report: { feedbackCenter?.show($0) }),
            lifecycle: row.summary.flatMap { summary in
                actions?.stacklyAction(for: summary, context: context)
            },
            open: { navigator?.openInstance($0) },
            openLogs: { navigator?.openInstance($0, tab: .logs) },
            showPlan: showPlan)
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

    private func detailsSection(_ detail: EnvironmentDetail) -> some View {
        let summary = detail.summary
        let lease = EnvironmentCopy.leaseText(
            detail.lease, now: fixtureNow ?? Date(), calendar: calendar, locale: locale,
            timeZone: timeZone)
        return CardSection("Details") {
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
            LabeledContent("Lock") { detailsValue(EnvironmentCopy.lockText(model.lock)) }
            LabeledContent("Local override") {
                detailsValue(EnvironmentCopy.localText(model.local))
            }
            if let lease {
                LabeledContent("Lease") {
                    Text(lease)
                        .multilineTextAlignment(.trailing)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .labeledContentStyle(.stackly)
    }

    /// A monospaced value of the Details section, as the mockup draws the file rows.
    private func detailsValue(_ text: String) -> some View {
        Text(text)
            .stacklyCode()
            .lineLimit(1)
            .truncationMode(.middle)
            .help(text)
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
