import AppKit
import SwiftUI

/// The main window's content: the sidebar with its sections, badges, and daemon footer,
/// the one daemon banner above every page, and the selected page.
///
/// It sets ``MutationAvailability`` for everything inside from the presentation, holds the
/// one ``ActionHost`` of the window (so a confirmation shows once), shows the feedback card
/// of the window's ``FeedbackCenter`` under the page, captures the scene's `openWindow`
/// action into the ``WindowOpener``, and reports when the main window is about to close. The
/// sidebar selection is restored per window by the system (`@SceneStorage`), Overview by
/// default.
public struct AppShell: View {
    /// The smallest window content size (spec 0013).
    static let minimumSize = CGSize(width: 900, height: 600)
    /// The sidebar column widths: the content column never gets below 640 pt.
    static let sidebarWidth = (min: CGFloat(200), ideal: CGFloat(236), max: CGFloat(260))
    /// The smallest content column width, also the gallery's narrow width.
    static let contentMinWidth: CGFloat = 640
    /// The content column width at the default window size of 1280 pt.
    static let contentIdealWidth: CGFloat = 1044

    @SceneStorage("shell.section") private var section: SidebarSection = .overview
    @State private var windowKey = MainWindowKey()
    @State private var windowWidth: CGFloat?
    @State private var navigator = ShellNavigator()
    @Environment(\.openWindow) private var openWindow
    @Environment(\.feedbackCenter) private var feedbackCenter

    private let presentation: ShellPresentation
    private let diagnostics: ConnectionDiagnostics
    private let counts: SidebarCounts
    private let instances: InstanceStore?
    private let catalog: CatalogStore?
    private let runtimes: RuntimeStore?
    private let operations: OperationCenter?
    private let capacity: CapacityModel?
    private let instanceActions: InstanceActionModel?
    private let newInstance: NewInstancePresenter?
    private let completedRounds: Int
    private let windowOpener: WindowOpener
    private let windowWillClose: (NSWindow) -> Void
    private let reconnect: @MainActor () -> Void
    private let refreshDiagnostics: @MainActor () -> Void
    private let onReselect: @MainActor (SidebarSection) -> Void
    /// Called with what the shell publishes to the menu bar; a test reads the host's gate.
    var onPublishCommands: (@MainActor (PublishedCommands) -> Void)?

    /// Creates the shell.
    ///
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
    ///   - operations: The operation center behind the Overview, or `nil` to show the
    ///     placeholder (previews).
    ///   - capacity: The disk reading behind the Overview, or `nil` for the placeholder.
    ///   - instanceActions: Start, Stop, and Retry for the Overview and Instances, or `nil`
    ///     for the placeholders.
    ///   - newInstance: Presents the New Instance sheet, or `nil` for a shell without one
    ///     (previews): then no page draws a New Instance button and ⌘N does nothing.
    ///   - completedRounds: How many snapshot rounds completed; the pages read it to end a
    ///     pending click and to read the disk again.
    ///   - windowOpener: Receives the scene's `openWindow` action.
    ///   - windowWillClose: Called with the main window right before it closes.
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
        operations: OperationCenter? = nil,
        capacity: CapacityModel? = nil,
        instanceActions: InstanceActionModel? = nil,
        newInstance: NewInstancePresenter? = nil,
        completedRounds: Int = 0,
        windowOpener: WindowOpener,
        windowWillClose: @escaping (NSWindow) -> Void,
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
        self.operations = operations
        self.capacity = capacity
        self.instanceActions = instanceActions
        self.newInstance = newInstance
        self.completedRounds = completedRounds
        self.windowOpener = windowOpener
        self.windowWillClose = windowWillClose
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
                isConnected: presentation.isConnected
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
            page
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                .background(StacklyColor.surfaceWindow)
                .safeAreaInset(edge: .top, spacing: 0) {
                    bannerArea
                }
                .feedbackPresenter()
                .navigationSplitViewColumnWidth(
                    min: Self.contentMinWidth,
                    ideal: Self.contentIdealWidth
                )
        }
        .frame(minWidth: Self.minimumSize.width, minHeight: Self.minimumSize.height)
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
        .environment(\.mainWindowKey, windowKey)
        .environment(\.shellNavigator, navigator)
        .environment(\.newInstanceAction, newInstance?.action)
        .environment(\.newInstancePresenter, newInstance)
        .modifier(NewInstanceSheetModifier(presenter: newInstance))
        // `@SceneStorage` only lives in a view, so the navigator mirrors it both ways.
        .onChange(of: section, initial: true) { _, new in
            if navigator.section != new { navigator.section = new }
        }
        .onChange(of: navigator.section) { _, new in
            if section != new { section = new }
        }
        .background(
            MainWindowAccessor(windowKey: windowKey, windowWillClose: windowWillClose)
        )
        .onAppear {
            windowOpener.capture(openWindow)
            connectNewInstance()
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

    /// The selection as the sidebar list needs it. The list can never clear it.
    private var selection: Binding<SidebarSection?> {
        Binding {
            section
        } set: { newValue in
            if let newValue {
                select(newValue)
            }
        }
    }

    /// Shows `newSection`. The one way to change the section: the sidebar, the commands
    /// ⌘1 to ⌘7, and Open Diagnostics in the banner all come here. Choosing the section that
    /// already shows calls ``onReselect`` instead.
    func select(_ newSection: SidebarSection) {
        if newSection == section {
            onReselect(newSection)
        } else {
            section = newSection
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

    @ViewBuilder private var bannerArea: some View {
        if let banner = presentation.banner {
            DaemonBanner(
                banner,
                isReconnecting: presentation.isReconnecting,
                reconnect: reconnect,
                openDiagnostics: { select(.diagnostics) }
            )
            // A new banner is a new view, so a new critical banner is announced again.
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
                    openDiagnostics: { select(.diagnostics) }
                )
            } else {
                UnbuiltScrollPage(section: .instances)
            }
        case .environments:
            // provisional: F44
            UnbuiltScrollPage(section: .environments)
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
            // provisional: F35
            UnbuiltScrollPage(
                section: .operations,
                subtitle: OperationCenter.subtitle(running: counts.runningOperations)
            )
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
    @Binding var selection: SidebarSection?
    let counts: SidebarCounts
    let isConnected: Bool

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
            SidebarRow(
                section: section,
                badge: section.badge(counts: counts, isConnected: isConnected)
            )
            .tag(section)
        }
    }
}

/// One sidebar row: the section icon and title, with the badge and its spoken value.
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
        .accessibilityIdentifier(AccessibilityID.Sidebar.row(section))
        if let badge {
            label.accessibilityValue(badge.spokenValue)
        } else {
            label
        }
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
