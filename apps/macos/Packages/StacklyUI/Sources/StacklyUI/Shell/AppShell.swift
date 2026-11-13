import AppKit
import StacklyClient
import SwiftUI

/// The main window's content: the sidebar with its sections, badges, and daemon footer,
/// the one daemon banner above every page, and the selected page.
/// It sets ``MutationAvailability`` for everything inside from the presentation, holds the
/// one ``ActionHost`` of the window (so a confirmation shows once), and shows the feedback
/// card of the window's ``FeedbackCenter`` under the page. The window itself (the
/// ``WindowOpener`` capture, the close report, the key state, and the minimum size) belongs to
/// ``MainWindowRoot``, which also decides between this shell and the onboarding. The sidebar
/// selection is restored per window by the system (`@SceneStorage`), Overview by default.
public struct AppShell: View {
    /// The smallest window content size (spec 0013).
    static let minimumSize = CGSize(width: 900, height: 600)
    /// The sidebar column widths: the content column never gets below 640 pt.
    static let sidebarWidth = (min: CGFloat(200), ideal: CGFloat(236), max: CGFloat(260))
    /// The smallest content column width, also the gallery's narrow width.
    static let contentMinWidth: CGFloat = 640
    /// The content column width at the default window size of 1280 pt.
    static let contentIdealWidth: CGFloat = 1044

    /// The sidebar row, restored per window: a section's raw value as before, or
    /// `environment:<uuid>` (spec 0044, *Sidebar*).
    @SceneStorage("shell.section") private var stored: SidebarSelection = .section(.overview)
    @State private var windowWidth: CGFloat?
    @State private var navigator = ShellNavigator()
    @Environment(\.feedbackCenter) private var feedbackCenter
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.shellDestinationQueue) private var destinationQueue
    @Environment(DaemonModel.self) private var daemon: DaemonModel?
    @Environment(NotificationModel.self) private var notifications: NotificationModel?

    private let presentation: ShellPresentation
    private let diagnostics: ConnectionDiagnostics
    private let counts: SidebarCounts
    private let instances: InstanceStore?
    private let catalog: CatalogStore?
    private let runtimes: RuntimeStore?
    private let environments: EnvironmentStore?
    private let operations: OperationCenter?
    private let capacity: CapacityModel?
    private let instanceActions: InstanceActionModel?
    private let newInstance: NewInstancePresenter?
    private let completedRounds: Int
    private let reconnect: @MainActor () -> Void
    private let refreshDiagnostics: @MainActor () -> Void
    private let onReselect: @MainActor (SidebarSection) -> Void
    /// Called with what the shell publishes to the menu bar; a test reads the host's gate.
    var onPublishCommands: (@MainActor (PublishedCommands) -> Void)?

    /// Creates the shell.
    /// - Parameters:
    ///   - presentation: What the shell shows for the daemon's state.
    ///   - diagnostics: The Connection section of Diagnostics.
    ///   - counts: The counts behind the sidebar badges.
    ///   - instances: The instance store behind the Instances page, or `nil` to show the
    ///     placeholder (previews).
    ///   - catalog: The catalog store behind the Catalog page, or `nil` to show the
    ///     placeholder (previews).
    ///   - runtimes: The runtime store behind the Runtimes page and the Get column of the
    ///     Catalog, or `nil` to show the placeholder (previews).
    ///   - environments: The environment store behind the sidebar's child rows, the
    ///     Environments page, and the Overview section, or `nil` to show the placeholder.
    ///   - operations: The operation center behind the Overview and the Operations page, or
    ///     `nil` to show the placeholder (previews).
    ///   - capacity: The disk reading behind the Overview, or `nil` for the placeholder.
    ///   - instanceActions: Start, Stop, and Retry for the Overview and Instances, or `nil`
    ///     for the placeholders.
    ///   - newInstance: Presents the New Instance sheet, or `nil` for a shell without one
    ///     (previews): then no page draws a New Instance button and ⌘N does nothing.
    ///   - completedRounds: How many snapshot rounds completed; the pages read it to end a
    ///     pending click and to read the disk again.
    ///   - reconnect: Tries to connect now (banner, Diagnostics).
    ///   - refreshDiagnostics: Asks the connected daemon for its info and health again.
    ///   - onReselect: Called when the section that already shows is chosen again, from the
    ///     sidebar or a command. Nothing by default; a page that has a way back fills it.
    public init(
        presentation: ShellPresentation,
        diagnostics: ConnectionDiagnostics,
        counts: SidebarCounts = SidebarCounts(),
        instances: InstanceStore? = nil,
        catalog: CatalogStore? = nil,
        runtimes: RuntimeStore? = nil,
        environments: EnvironmentStore? = nil,
        operations: OperationCenter? = nil,
        capacity: CapacityModel? = nil,
        instanceActions: InstanceActionModel? = nil,
        newInstance: NewInstancePresenter? = nil,
        completedRounds: Int = 0,
        reconnect: @escaping @MainActor () -> Void,
        refreshDiagnostics: @escaping @MainActor () -> Void,
        onReselect: @escaping @MainActor (SidebarSection) -> Void = { _ in }
    ) {
        self.presentation = presentation
        self.diagnostics = diagnostics
        self.counts = counts
        self.instances = instances
        self.catalog = catalog
        self.runtimes = runtimes
        self.environments = environments
        self.operations = operations
        self.capacity = capacity
        self.instanceActions = instanceActions
        self.newInstance = newInstance
        self.completedRounds = completedRounds
        self.reconnect = reconnect
        self.refreshDiagnostics = refreshDiagnostics
        self.onReselect = onReselect
    }

    /// The shell's content.
    public var body: some View {
        NavigationSplitView {
            ShellSidebar(
                selection: selection,
                counts: counts,
                isConnected: presentation.isConnected,
                environments: environments?.environments ?? [],
                reselect: { select($0) }
            )
            .navigationSplitViewColumnWidth(
                min: Self.sidebarWidth.min,
                ideal: Self.sidebarWidth.ideal,
                max: Self.sidebarWidth.max
            )
            .safeAreaInset(edge: .bottom, spacing: 0) {
                DaemonFooter(status: presentation.footer)
            }
        } detail: {
            ShellPageLayout(page: page, banner: bannerArea)
                // The nested navigation stack's toolbar backdrop otherwise follows the
                // page down and blurs its controls below the banner.
                .toolbarBackgroundVisibility(
                    presentation.banner == nil && restartBanner == nil ? .automatic : .hidden,
                    for: .windowToolbar
                )
                .feedbackPresenter()
                .task(id: daemon?.restart.toast?.id) { showRestartToast() }
                .navigationSplitViewColumnWidth(
                    min: Self.contentMinWidth,
                    ideal: Self.contentIdealWidth
                )
        }
        // The inspector's widest column depends on the window width (spec 0067, AC-18).
        .onGeometryChange(for: CGFloat.self) {
            $0.size.width
        } action: {
            windowWidth = $0
        }
        .environment(\.stacklyWindowWidth, windowWidth)
        // The commands read the host, and the host reads the gate from the environment it
        // sits in, so each goes before (inside) the line that sets what it reads: spec 0069.
        .modifier(
            PublishCommandsModifier(
                level: .shell, actions: shellActions, checked: [],
                onPublish: onPublishCommands)
        )
        .actionHost()
        .environment(\.mutationAvailability, presentation.mutationAvailability)
        .environment(\.shellNavigator, navigator)
        .environment(\.newInstanceAction, newInstance?.action)
        .environment(\.duplicateInstanceAction, newInstance?.duplicateAction)
        .environment(\.newInstancePresenter, newInstance)
        .modifier(NewInstanceSheetModifier(presenter: newInstance))
        // `@SceneStorage` only lives in a view, so the navigator mirrors it both ways.
        .onChange(of: stored, initial: true) { _, new in
            if navigator.selection != new { navigator.selection = new }
        }
        .onChange(of: navigator.selection) { _, new in
            if stored != new { stored = new }
        }
        // An environment that is gone falls back to the Environments list (spec 0044).
        .onChange(of: environmentGone, initial: true) { _, gone in
            if gone { navigator.selection = .section(.environments) }
        }
        // The menu bar popover leaves a destination here, also while this window opens.
        .onChange(of: destinationQueue?.pending, initial: true) { _, pending in
            guard pending != nil, let destination = destinationQueue?.take() else { return }
            navigator.show(destination)
        }
        .onAppear {
            connectNewInstance()
        }
        // A click on a notification asks for a page; the window shows it (spec 0032 AC-22).
        .onChange(of: notifications?.pending?.id, initial: true) {
            if let destination = notifications?.takePending() {
                navigator.show(destination)
            }
        }
        .task(id: runtimes.map(ObjectIdentifier.init)) {
            await showFinishedInstalls()
        }
    }

    /// Tells the New Instance presenter what to do when a create succeeded: show the new
    /// instance and report the result in this window's feedback card (spec 0020, *Selesai*).
    private func connectNewInstance() {
        guard let newInstance else { return }
        let navigator = navigator
        let feedbackCenter = feedbackCenter
        newInstance.finishHandler = { outcome in
            if let id = outcome.instanceID {
                navigator.show(.instances(filter: .all, select: id))
            }
            feedbackCenter?.show(outcome.feedback)
        }
    }

    /// Reports how each install the person started ended, also after they left the Catalog
    /// (spec 0018, *Kolom aksi Catalog*). A failure shows in the detail panel, not here.
    private func showFinishedInstalls() async {
        guard let runtimes else { return }
        for await finished in runtimes.finishedInstalls {
            if let feedback = finished.feedback {
                feedbackCenter?.show(feedback)
            }
        }
    }

    /// The section that shows.
    private var section: SidebarSection {
        stored.section
    }

    /// Whether the selected environment is not in the loaded list any more.
    private var environmentGone: Bool {
        guard case .environment(let id) = stored, let list = environments?.environments else {
            return false
        }
        return list.contains { $0.id == id } == false
    }

    /// The selection as the sidebar list needs it. The list can never clear it.
    private var selection: Binding<SidebarSelection?> {
        Binding {
            stored
        } set: { newValue in
            switch newValue {
            case .section(let newSection): select(newSection)
            case .environment(let id): stored = .environment(id)
            case nil: break
            }
        }
    }

    /// Shows `newSection`. The one way to change the section: the sidebar, the commands
    /// ⌘1 to ⌘7, and Open Diagnostics in the banner all come here. Choosing the section that
    /// already shows calls ``onReselect`` instead.
    func select(_ newSection: SidebarSection) {
        if stored == .section(newSection) {
            // The detail page of an instance is a page of the Instances section; choosing
            // the section again goes back to its table (spec 0021).
            if newSection == .instances { navigator.showInstancesTable() }
            onReselect(newSection)
        } else {
            // Environments from one of its environments goes back to the list (spec 0044).
            stored = .section(newSection)
        }
    }

    /// The actions the shell publishes to the menu bar.
    private var shellActions: [CommandID: StacklyAction] {
        Self.shellActions(
            presentation: presentation,
            select: { select($0) },
            reconnect: reconnect,
            newInstance: newInstance?.action.stacklyAction(
                title: CommandMap.entry(for: .newInstance).title)
        )
    }

    /// The restart banner and the model that dismisses it, when it takes the banner slot:
    /// while the daemon restarts it replaces the daemon banner, which would only say that
    /// stacklyd is gone; once the plan ended it gives way to a daemon banner.
    private var restartBanner: (content: RestartBannerContent, model: DaemonRestartModel)? {
        guard let model = daemon?.restart,
            let content = model.banner(
                now: fixtureNow ?? Date(), daemonBannerShown: presentation.banner != nil)
        else { return nil }
        return (content, model)
    }

    /// Shows the toast of a restart that finished, once.
    private func showRestartToast() {
        guard let restart = daemon?.restart, let toast = restart.toast else { return }
        feedbackCenter?.show(.success(toast.message))
        restart.consumeToast(toast.id)
    }

    @ViewBuilder private var bannerArea: some View {
        if let (content, model) = restartBanner {
            RestartBanner(content, dismiss: { model.dismissBanner() })
                .id(content.title)
                .frame(minWidth: Self.contentMinWidth - 2 * Spacing.pageHorizontal)
                .padding(.horizontal, Spacing.pageHorizontal)
                .padding(.vertical, Spacing.s12)
                .background(StacklyColor.surfaceWindow)
                .transition(StacklyMotion.crossfade)
        } else if let banner = presentation.banner {
            DaemonBanner(
                banner,
                isReconnecting: presentation.isReconnecting,
                reconnect: reconnect,
                openDiagnostics: { select(.diagnostics) }
            )

            .id(banner.title)
            // The split view also measures the banner at a tiny width, where its text would
            // wrap one character per line and push the columns out of the window.
            .frame(minWidth: Self.contentMinWidth - 2 * Spacing.pageHorizontal)
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s12)
            .background(StacklyColor.surfaceWindow)
            .transition(StacklyMotion.crossfade)
        }
    }

    @ViewBuilder private var page: some View {
        switch section {
        case .overview:
            if let instances, let operations, let capacity, let instanceActions {
                OverviewPage(
                    instances: instances,
                    catalog: catalog,
                    runtimes: runtimes,
                    environments: environments,
                    operations: operations,
                    capacity: capacity,
                    actions: instanceActions,
                    isConnected: presentation.isConnected,
                    completedRounds: completedRounds,
                    waiting: presentation.pageFailure,
                    hasConnection: presentation.hasConnection,
                    reconnect: reconnect,
                    openDiagnostics: { select(.diagnostics) }
                )
            } else {
                UnbuiltScrollPage(section: .overview)
            }
        case .instances:
            if let instances, let instanceActions {
                InstancesPage(
                    instances: instances,
                    catalog: catalog,
                    actions: instanceActions,
                    operations: operations,
                    completedRounds: completedRounds,
                    waiting: presentation.pageFailure,
                    hasConnection: presentation.hasConnection,
                    reconnect: reconnect,
                    openDiagnostics: { select(.diagnostics) },
                    detail: daemon.map(InstanceDetailService.live),
                    logs: daemon.map(InstanceLogsService.live),
                    metrics: daemon.map(InstanceMetricsService.live),
                    config: daemon.map(InstanceConfigService.live)
                )
            } else {
                UnbuiltScrollPage(section: .instances)
            }
        case .environments:
            if let environments {
                EnvironmentsPage(
                    environments: environments,
                    instances: instances,
                    catalog: catalog,
                    actions: instanceActions,
                    selected: navigator.environment,
                    waiting: presentation.pageFailure,
                    hasConnection: presentation.hasConnection,
                    reconnect: reconnect,
                    openDiagnostics: { select(.diagnostics) },
                    detail: daemon.map(InstanceDetailService.live)
                )
            } else {
                UnbuiltScrollPage(section: .environments)
            }
        case .catalog:
            if let catalog {
                CatalogPage(
                    catalog: catalog,
                    runtimes: runtimes,
                    operations: operations,
                    waiting: presentation.pageFailure,
                    hasConnection: presentation.hasConnection,
                    reconnect: reconnect,
                    openDiagnostics: { select(.diagnostics) }
                )
            } else {
                UnbuiltScrollPage(section: .catalog)
            }
        case .runtimes:
            if let runtimes {
                RuntimesPage(
                    runtimes: runtimes,
                    catalog: catalog,
                    operations: operations,
                    waiting: presentation.pageFailure,
                    hasConnection: presentation.hasConnection,
                    reconnect: reconnect,
                    openDiagnostics: { select(.diagnostics) }
                )
            } else {
                UnbuiltScrollPage(section: .runtimes)
            }
        case .operations:
            if let operations {
                OperationsPage(
                    operations: operations,
                    instances: instances,
                    runtimes: runtimes,
                    actions: instanceActions,
                    service: daemon.map(OperationsService.live) ?? .disconnected,
                    waiting: presentation.pageFailure,
                    hasConnection: presentation.hasConnection,
                    reconnect: reconnect,
                    openDiagnostics: { select(.diagnostics) }
                )
            } else {
                UnbuiltScrollPage(
                    section: .operations,
                    subtitle: OperationCenter.subtitle(running: counts.runningOperations)
                )
            }
        case .diagnostics:
            DiagnosticsPage(
                diagnostics: diagnostics,
                isReconnecting: presentation.isReconnecting,
                reconnect: reconnect,
                refresh: refreshDiagnostics
            )
        }
    }
}

/// Keeps the window banner outside the page's native navigation and inspector containers.
struct ShellPageLayout<Page: View, Banner: View>: View {
    let page: Page
    let banner: Banner

    var body: some View {
        // Native inspector containers drop an ancestor's safe area inset, even when
        // closed. Reserve the banner's height in the layout instead of the safe area.
        VStack(spacing: 0) {
            banner
            page
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                .background(StacklyColor.surfaceWindow)
        }
    }
}

/// An unbuilt page in a scroll view, with the section title in the toolbar and, for
/// Operations, the subtitle the operation center gives.
private struct UnbuiltScrollPage: View {
    let section: SidebarSection
    var subtitle: String?

    var body: some View {
        ScrollView {
            UnbuiltPage(section: section)
        }
        .pageChrome(title: section.title, subtitle: subtitle)
    }
}

/// The sidebar: the main sections, then Library and System, each row with its section
/// icon and an optional native badge.
private struct ShellSidebar: View {
    @Binding var selection: SidebarSelection?
    let counts: SidebarCounts
    let isConnected: Bool
    /// The environments, by name, as child rows under Environments (spec 0044).
    let environments: [EnvironmentSummary]
    @State private var environmentsExpanded = true
    /// Called when the row of the section that already shows is clicked again: a `List`
    /// does not set a selection that did not change.
    let reselect: (SidebarSection) -> Void

    var body: some View {
        List(selection: $selection) {
            ForEach(SidebarGroup.allCases) { group in
                if let title = group.title {
                    Section(title) {
                        rows(of: group)
                    }
                } else {
                    Section {
                        rows(of: group)
                    }
                }
            }
        }
        .listStyle(.sidebar)
    }

    private func rows(of group: SidebarGroup) -> some View {
        ForEach(group.sections) { section in
            if section == .environments, environments.isEmpty == false {
                // Open from the start; the Environments row itself has no badge.
                DisclosureGroup(isExpanded: $environmentsExpanded) {
                    ForEach(environments) { environment in
                        EnvironmentSidebarRow(environment: environment)
                            .tag(SidebarSelection.environment(environment.id))
                    }
                } label: {
                    row(of: section)
                }
            } else {
                row(of: section)
            }
        }
    }

    private func row(of section: SidebarSection) -> some View {
        SidebarRow(
            section: section,
            badge: section.badge(counts: counts, isConnected: isConnected)
        )
        .tag(SidebarSelection.section(section))
        .simultaneousGesture(
            TapGesture().onEnded {
                if selection == .section(section) { reselect(section) }
            })
    }
}

/// One environment under Environments: its status symbol, its name, and "{ready}/{total}"
/// (spec 0044, *Sidebar*).
private struct EnvironmentSidebarRow: View {
    let environment: EnvironmentSummary

    var body: some View {
        let appearance = StatusAppearance.environment(status: environment.status)
        Label {
            HStack {
                Text(environment.name)
                    .lineLimit(1)
                Spacer(minLength: Spacing.s4)
                Text("\(environment.readyCount)/\(environment.serviceCount)")
                    .monospacedDigit()
                    .foregroundStyle(StacklyColor.textSecondary)
            }
        } icon: {
            Image(systemName: appearance.symbol)
                .foregroundStyle(appearance.tone.fill)
        }
    }
}

private struct SidebarRow: View {
    let section: SidebarSection
    let badge: SidebarBadge?

    var body: some View {
        let label = Label {
            Text(section.title)
        } icon: {
            SectionIcon(section.style)
        }
        .badge(badge?.count ?? 0)
        .badgeProminence(badge?.isProminent == true ? .increased : .standard)

        label
    }
}

#if DEBUG
    #Preview("Light") {
        ShellPreviewSamples.shell(ShellPreviewSamples.unreachable)
            .frame(width: 1280, height: 800)
            .preferredColorScheme(.light)
    }

    #Preview("Dark") {
        ShellPreviewSamples.shell(ShellPresentation(status: .connected(.ready)))
            .frame(width: 1280, height: 800)
            .preferredColorScheme(.dark)
    }

    #Preview("Narrow width") {
        ShellPreviewSamples.shell(
            ShellPresentation(
                status: .incompatible(
                    .majorMismatch(daemonProtocol: "2.0", appProtocol: "1.0"),
                    since: ShellPreviewSamples.now
                )
            )
        )
        .frame(width: AppShell.minimumSize.width, height: AppShell.minimumSize.height)
        .preferredColorScheme(.light)
    }

    #Preview("Diagnostics, narrow content") {
        ScrollView {
            ConnectionDiagnosticsSection(
                ShellPreviewSamples.diagnostics,
                isReconnecting: false,
                reconnect: {},
                refresh: {}
            )
            .padding(Spacing.pageHorizontal)
        }
        .frame(width: AppShell.contentMinWidth, height: 600)
        .background(StacklyColor.surfaceWindow)
    }
#endif
