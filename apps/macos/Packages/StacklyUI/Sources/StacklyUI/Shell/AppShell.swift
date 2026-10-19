import AppKit
import SwiftUI

/// The main window's content: the sidebar with its sections, badges, and daemon footer,
/// the one daemon banner above every page, and the selected page.
///
/// It sets ``MutationAvailability`` for everything inside from the presentation, captures
/// the scene's `openWindow` action into the ``WindowOpener``, and reports when the main
/// window is about to close. The sidebar selection is restored per window by the system
/// (`@SceneStorage`), Overview by default.
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
    @Environment(\.openWindow) private var openWindow

    private let presentation: ShellPresentation
    private let diagnostics: ConnectionDiagnostics
    private let counts: SidebarCounts
    private let windowOpener: WindowOpener
    private let windowWillClose: (NSWindow) -> Void
    private let reconnect: @MainActor () -> Void
    private let refreshDiagnostics: @MainActor () -> Void

    /// Creates the shell.
    ///
    /// - Parameters:
    ///   - presentation: What the shell shows for the daemon's state.
    ///   - diagnostics: The Connection section of Diagnostics.
    ///   - counts: The counts behind the sidebar badges.
    ///   - windowOpener: Receives the scene's `openWindow` action.
    ///   - windowWillClose: Called with the main window right before it closes.
    ///   - reconnect: Tries to connect now (banner, Diagnostics).
    ///   - refreshDiagnostics: Asks the connected daemon for its info and health again.
    public init(
        presentation: ShellPresentation,
        diagnostics: ConnectionDiagnostics,
        counts: SidebarCounts = SidebarCounts(),
        windowOpener: WindowOpener,
        windowWillClose: @escaping (NSWindow) -> Void,
        reconnect: @escaping @MainActor () -> Void,
        refreshDiagnostics: @escaping @MainActor () -> Void
    ) {
        self.presentation = presentation
        self.diagnostics = diagnostics
        self.counts = counts
        self.windowOpener = windowOpener
        self.windowWillClose = windowWillClose
        self.reconnect = reconnect
        self.refreshDiagnostics = refreshDiagnostics
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
                .navigationSplitViewColumnWidth(
                    min: Self.contentMinWidth,
                    ideal: Self.contentIdealWidth
                )
        }
        .frame(minWidth: Self.minimumSize.width, minHeight: Self.minimumSize.height)
        .environment(\.mutationAvailability, presentation.mutationAvailability)
        .background(MainWindowAccessor(windowWillClose: windowWillClose))
        .onAppear {
            windowOpener.capture(openWindow)
        }
    }

    /// The selection as the sidebar list needs it. The list can never clear it.
    private var selection: Binding<SidebarSection?> {
        Binding {
            section
        } set: { newValue in
            if let newValue {
                section = newValue
            }
        }
    }

    @ViewBuilder private var bannerArea: some View {
        if let banner = presentation.banner {
            DaemonBanner(
                banner,
                isReconnecting: presentation.isReconnecting,
                reconnect: reconnect,
                openDiagnostics: { section = .diagnostics }
            )
            // A new banner is a new view, so a new critical banner is announced again.
            .id(banner.title)
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s12)
            .background(StacklyColor.surfaceWindow)
            .transition(StacklyMotion.crossfade)
        }
    }

    @ViewBuilder private var page: some View {
        switch section {
        case .overview:
            // provisional: F22
            UnbuiltScrollPage(section: .overview)
        case .instances:
            // provisional: F13 (milestone 4 shows the provisional instance list here)
            UnbuiltScrollPage(section: .instances)
        case .environments:
            // provisional: F44
            UnbuiltScrollPage(section: .environments)
        case .catalog:
            // provisional: F17
            UnbuiltScrollPage(section: .catalog)
        case .runtimes:
            // provisional: F18
            UnbuiltScrollPage(section: .runtimes)
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
        if let badge {
            label.accessibilityValue(badge.spokenValue)
        } else {
            label
        }
    }
}

#if DEBUG
    /// Sample presentations for the shell previews.
    enum ShellPreviewSamples {
        static let now = Date(timeIntervalSince1970: 1_792_300_000)

        static let unreachable = ShellPresentation(
            status: .unreachable(lastKnownAt: now.addingTimeInterval(-300), since: now),
            lastErrorCode: "ipc.endpoint_missing"
        )

        static let diagnostics = ConnectionDiagnostics(
            status: .unreachable(lastKnownAt: now.addingTimeInterval(-300), since: now),
            socketPath:
                "/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/stackly-1a2b3c4d/stacklyd.sock",
            lastError: DiagnosticsError(
                code: "ipc.endpoint_missing",
                category: "unavailable",
                message: "no socket at the daemon path"
            ),
            nextAttempt: now.addingTimeInterval(5),
            lastConnectedAt: now.addingTimeInterval(-300),
            daemon: DaemonFacts(
                version: "0.1.0",
                pid: 4242,
                startedAt: now.addingTimeInterval(-7_200),
                serverInstanceId: "0192f4a1-7c1e-7b3a-9d2e-5f6a7b8c9d0e"
            ),
            daemonProtocol: "1.0",
            appProtocol: "1.0",
            contractDrift: false
        )

        static func shell(_ presentation: ShellPresentation) -> some View {
            AppShell(
                presentation: presentation,
                diagnostics: diagnostics,
                counts: SidebarCounts(instances: 6, runningOperations: 2),
                windowOpener: WindowOpener(),
                windowWillClose: { _ in },
                reconnect: {},
                refreshDiagnostics: {}
            )
        }
    }

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
