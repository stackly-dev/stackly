import StacklyClient
import SwiftUI

/// The detail page of one instance (spec 0021, *Halaman detail dan navigasi*): pushed onto
/// the navigation stack of the Instances page. The header and the rows of the summary come
/// from the ``InstanceStore`` at once; the rows that need the detail wait for
/// `instance.get_detail`.
/// The toolbar holds the main action of the instance and the More menu. An instance the
/// store does not hold is looked up once; one the daemon does not know, or one that was
/// removed while the page was open, shows "Instance Not Found" with a way back.
struct InstanceDetailPage: View {
    /// Where the lookup of an instance the store does not hold stands.
    private enum Lookup: Equatable {
        case searching
        case missing
        case failed(ErrorBody)
    }

    @Environment(\.feedbackCenter) private var feedbackCenter
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.duplicateInstanceAction) private var duplicateInstance
    @Environment(\.loadingDelay) private var loadingDelay
    @State private var lookup = Lookup.searching
    @State private var hadSummary = false
    @State private var tab: InstanceTab
    @State private var secretNeeds: [SecretNeed]?
    @State private var openInNeeds: [SecretNeed]?

    let id: UUID
    let instances: InstanceStore
    let catalog: CatalogStore?
    let actions: InstanceActionModel
    let model: InstanceDetailModel
    let service: InstanceDetailService
    let openIn: OpenInModel
    let logs: InstanceLogsModel
    let metrics: InstanceMetricsModel
    let configuration: ConfigurationModel
    let completedRounds: Int
    let back: @MainActor () -> Void

    init(
        id: UUID, instances: InstanceStore, catalog: CatalogStore?, actions: InstanceActionModel,
        model: InstanceDetailModel, service: InstanceDetailService, openIn: OpenInModel,
        logs: InstanceLogsModel, metrics: InstanceMetricsModel, configuration: ConfigurationModel,
        completedRounds: Int,
        tab: InstanceTab = .summary,
        back: @escaping @MainActor () -> Void
    ) {
        self.id = id
        self.instances = instances
        self.catalog = catalog
        self.actions = actions
        self.model = model
        self.service = service
        self.openIn = openIn
        self.logs = logs
        self.metrics = metrics
        self.configuration = configuration
        self.completedRounds = completedRounds
        _tab = State(initialValue: tab)
        self.back = back
    }

    private var context: InstanceActionContext {
        InstanceActionContext.make(
            feedbackCenter: feedbackCenter, navigator: navigator,
            leftoverProcesses: { [instances] in instances.store.items[$0]?.leftoverProcesses ?? [] }
        )
    }

    var body: some View {
        Group {
            if let summary = instances.store.items[id], model.isNotFound == false {
                page(summary)
            } else {
                missing
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(StacklyColor.surfaceWindow)

        .task(id: id) { await locate() }
    }

    // MARK: - Page

    private func page(_ summary: InstanceSummary) -> some View {
        let action = actions.stacklyAction(for: summary, context: context)
        let lifecycle = actions.lifecycleActions(
            for: summary, providerName: providerName(summary), context: context)
        return Group {
            switch tab {
            case .summary:
                ScrollView {
                    VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                        tabPicker
                        SummaryTab(
                            summary: summary, action: action, model: model,
                            secrets: service.secrets,
                            home: NSHomeDirectory(), enterSecrets: { secretNeeds = $0 },
                            stopping: Self.stoppingActions(
                                summary: summary, local: actions.localStatus(for: summary.id),
                                stopAgain: {
                                    actions.perform(.stop, for: summary, context: context)
                                },
                                forceStop: lifecycle.forceStop))
                    }
                    .padding(.horizontal, Spacing.pageHorizontal)
                    .padding(.vertical, Spacing.s16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            case .configuration:
                ScrollView {
                    VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                        tabPicker
                        ConfigurationTab(model: configuration, summary: summary, context: context)
                    }
                    .padding(.horizontal, Spacing.pageHorizontal)
                    .padding(.vertical, Spacing.s16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            case .logs:
                VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                    tabPicker
                    InstanceLogsTab(model: logs, completedRounds: completedRounds)
                }
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.vertical, Spacing.s16)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            case .metrics:
                ScrollView {
                    VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                        tabPicker
                        InstanceMetricsTab(
                            model: metrics, instanceName: summary.name, action: action,
                            completedRounds: completedRounds)
                    }
                    .padding(.horizontal, Spacing.pageHorizontal)
                    .padding(.vertical, Spacing.s16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
        .pageChrome(title: summary.name, subtitle: subtitle(summary))
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                ActionButton(action, showsReason: false)
                    .help(CommandMap.helpText(for: Self.command(of: action), title: action.title))
            }
            if openInContent.showsButton {
                ToolbarItem(placement: .primaryAction) {
                    OpenInMenu(content: openInContent)
                }
            }
            ToolbarItem(placement: .primaryAction) {
                lifecycleButton(lifecycle.restart)
            }
            ToolbarItem(placement: .primaryAction) {
                lifecycleButton(lifecycle.reload)
            }
            ToolbarItem(placement: .primaryAction) {
                ActionMenu([moreItems(summary)])
            }
        }
        .task(id: id) { await model.open() }
        .task(id: id) { await model.watchSecrets() }
        .onChange(of: summary, initial: true) { _, new in
            hadSummary = true
            model.observe(new)
        }
        .onDisappear { model.close() }
        .openInSecretSheet(
            needs: $openInNeeds, secrets: service.secrets,
            reload: { Task { await model.reload() } }
        )
        .sheet(
            isPresented: Binding(
                get: { secretNeeds != nil }, set: { if $0 == false { secretNeeds = nil } })
        ) {
            SecretEntrySheet(
                needs: secretNeeds ?? [], client: service.secrets, actionTitle: "Start Instance",
                onContinue: {
                    secretNeeds = nil
                    actions.stacklyAction(for: summary, context: context).perform()
                },
                onCancel: { secretNeeds = nil })
        }
    }

    /// What the Open In button of the toolbar shows: the actions of the detail this page
    /// reads (spec 0070, AC-5).
    private var openInContent: OpenInContent {
        openIn.content(
            summary: instances.store.items[id], detail: model.detail,
            report: { feedbackCenter?.show($0) },
            enterSecret: { openInNeeds = [$0] },
            reload: { Task { await model.reload() } })
    }

    @ViewBuilder private var tabPicker: some View {
        let tabs = InstanceTab.allCases
        if tabs.count > 1 {
            Picker("Tab", selection: $tab) {
                ForEach(tabs) { tab in
                    Text(tab.title).tag(tab)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

        }
    }

    /// The items of the More menu of the toolbar.
    private func moreItems(_ summary: InstanceSummary) -> [StacklyAction] {
        Self.moreItems(
            copyConnection: copyConnection, duplicate: duplicateInstance,
            source: DuplicateSourceRef(id: summary.id, name: summary.name))
    }

    /// The items of the More menu: Copy Connection String, then Duplicate… when the window has the sheet for it (spec 0024 AC-13).
    /// - Parameters:
    ///   - copyConnection: The Copy Connection String action.
    ///   - duplicate: Opens the sheet on an instance, or `nil` where the window has none.
    ///   - source: The instance of the page.
    static func moreItems(
        copyConnection: StacklyAction, duplicate: DuplicateInstanceAction?,
        source: DuplicateSourceRef
    ) -> [StacklyAction] {
        var items = [copyConnection]
        if let duplicate {
            items.append(duplicate.stacklyAction(for: source))
        }
        return items
    }

    private var copyConnection: StacklyAction {
        ConnectionCopier.action(
            detail: model.detail, secrets: service.secrets,
            report: { feedbackCenter?.show($0) })
    }

    /// The Instance menu command a main action stands for, for the tooltip of its button:
    /// Stop is `stopInstance`; Start, Retry, and a busy start are `startInstance`.
    static func command(of action: StacklyAction) -> CommandID {
        switch action.id {
        case InstanceVerb.stop.rawValue: .stopInstance
        case InstanceVerb.restart.rawValue: .restartInstance
        case InstanceVerb.reload.rawValue: .reloadInstance
        case InstanceVerb.forceStop.rawValue: .forceStopInstance
        default: .startInstance
        }
    }

    /// An icon button of the toolbar for Restart or Reload, with the help text of spec 0026
    /// (*Copy*): the title, or the reason it cannot run, or the busy label.
    private func lifecycleButton(_ action: StacklyAction) -> some View {
        ActionButton(action, showsReason: false)
            .labelStyle(.iconOnly)
            .help(
                action.disabledReason
                    ?? (action.isBusy
                        ? action.title
                        : action.id == InstanceVerb.reload.rawValue
                            ? "Reload configuration" : "Restart"))
    }

    /// The actions of the "Still stopping." banner (spec 0026, AC-16): only while the
    /// instance is `stopping` and this window runs no click for it, since a click shows its
    /// busy label instead.
    static func stoppingActions(
        summary: InstanceSummary, local: InstanceLocalStatus,
        stopAgain: @escaping @MainActor () -> Void, forceStop: StacklyAction
    ) -> InstanceDetailBanner.Stopping? {
        guard summary.lifecycle == .stopping, local == .none else { return nil }
        return InstanceDetailBanner.Stopping(stopAgain: stopAgain, forceStop: forceStop)
    }

    /// The provider's display name: the detail's, then the catalog's, then the software.
    private func providerName(_ summary: InstanceSummary) -> String {
        model.detail?.provider.displayName
            ?? catalog?.providers?.first { $0.provider == summary.provider.name }?.displayName
            ?? summary.software
    }

    /// "{provider} {version} · {status}".
    private func subtitle(_ summary: InstanceSummary) -> String {
        let provider = providerName(summary)
        let version = summary.runtimeVersion.isEmpty ? "" : " \(summary.runtimeVersion)"
        return "\(provider)\(version) · \(StatusAdapter.appearance(of: summary).label)"
    }

    // MARK: - Missing

    @ViewBuilder private var missing: some View {
        Group {
            switch lookup {
            case .searching where hadSummary == false && model.isNotFound == false:
                DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
            case .failed(let body) where hadSummary == false:
                StatePlaceholder(
                    FailureContent(
                        title: "Couldn’t Load Instance", message: body.message, code: body.code),
                    retry: { Task { await locate() } },
                    openDiagnostics: { navigator?.show(.diagnostics) })
            default:
                StatePlaceholder(
                    EmptyContent.noData(
                        symbol: SidebarSection.instances.style.symbol,
                        title: "Instance Not Found",
                        message: hadSummary
                            ? "This instance was removed." : "It may have been removed.",
                        primaryAction: StateAction(
                            title: "Back to Instances", mutates: false, perform: back))
                )

            }
        }
        .pageChrome(title: "Instance")
    }

    /// Looks for the instance once when the store does not hold it.
    private func locate() async {
        guard instances.store.items[id] == nil else { return }
        lookup = .searching
        do throws(ClientError) {
            if let found = try await service.instance(id) {
                instances.offer(found)
            } else {
                lookup = .missing
            }
        } catch {
            lookup = error.body.code == "resource.not_found" ? .missing : .failed(error.body)
        }
    }
}
