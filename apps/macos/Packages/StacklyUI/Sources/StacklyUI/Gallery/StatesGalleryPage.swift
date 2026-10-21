#if DEBUG
    import SwiftUI

    /// The States page of the design gallery: every `ViewState` shape built with
    /// `ViewState.resolve`, live shape changes with the delayed spinner, placeholders,
    /// banners per tone, and the mutation gate, with sample data from the mockup.
    struct StatesGalleryPage: View {
        /// A group of sections of the page. The snapshot suites render the groups one by
        /// one, because the whole page is taller than a snapshot may be (spec 0068).
        enum Part: CaseIterable {
            /// The controls and every `ViewState` shape.
            case shapes
            /// The live shape changes with the delayed spinner.
            case live
            /// The placeholders.
            case placeholders
            /// The banners per tone and the mutation gate.
            case banners
        }

        @State private var isDaemonUnreachable = false
        @State private var isRefreshing = false
        private let parts: Set<Part>

        /// Creates the page with the sections of `parts`; every part by default.
        init(parts: Set<Part> = Set(Part.allCases)) {
            self.parts = parts
        }

        var body: some View {
            GalleryPageLayout {
                if parts.contains(.shapes) {
                    ControlsSection(
                        isDaemonUnreachable: $isDaemonUnreachable,
                        isRefreshing: $isRefreshing
                    )
                    ShapesSection(isRefreshing: isRefreshing)
                }
                if parts.contains(.live) {
                    LiveSection()
                }
                if parts.contains(.placeholders) {
                    PlaceholderSection()
                }
                if parts.contains(.banners) {
                    BannerSection()
                    GateSection()
                }
            }
            .environment(\.mutationAvailability, availability)
        }

        private var availability: MutationAvailability {
            isDaemonUnreachable
                ? .blocked(reason: StatesSample.offlineReason, since: StatesSample.lastKnown)
                : .available
        }
    }

    // MARK: - Sample data

    /// Sample data from the mockup. No real credentials.
    private enum StatesSample {
        static let lastKnown = FixtureClock.minutesAgo(18)
        static let catalogUpdated = FixtureClock.minutesAgo(26 * 60)
        static let offlineReason =
            "stacklyd is unavailable. Actions are disabled until the daemon is reachable."
        static let instances = [
            SampleInstance(name: "pg17-dev", endpoint: "127.0.0.1:5432"),
            SampleInstance(name: "mysql-erp", endpoint: "127.0.0.1:3306"),
            SampleInstance(name: "nats-erp", endpoint: "127.0.0.1:4222"),
        ]
        static let loadFailure = FailureContent(
            title: "Couldn’t Load Instances",
            message: "the instance registry could not be read",
            code: "E_REGISTRY_UNAVAILABLE"
        )
        static let backupsUnsupported = UnsupportedContent(
            title: "Backups Aren’t Supported",
            reason: "The Valkey provider doesn’t declare a backup capability, so Stackly won’t "
                + "guess at a file copy. Use the server’s own tools if you need snapshots."
        )
        static let backupReason = "The Valkey provider doesn’t declare a backup capability."

        /// No Instances with Browse Catalog and New Instance… (mockup Instances page).
        static func noData(_ value: [SampleInstance]) -> EmptyContent? {
            guard value.isEmpty else { return nil }
            return .noData(
                symbol: "server.rack",
                title: "No Instances",
                message: "Each instance gets its own data directory, port and secret. Start by "
                    + "installing a runtime.",
                primaryAction: StateAction(title: "New Instance…", mutates: true) {},
                secondaryAction: StateAction(title: "Browse Catalog", mutates: false) {}
            )
        }

        /// No results for “mysql-legacy-archive” (mockup Instances filter).
        static func noResults(_ value: [SampleInstance]) -> EmptyContent? {
            if let empty = noData(value) { return empty }
            return .noResults(
                query: "mysql-legacy-archive",
                message: "Check the spelling or try another status.",
                clearFilters: {}
            )
        }

        /// 2 of 9 services ready, with seven items so Show All appears.
        static let partial = PartialNote(
            summary: "2 of 9 services ready",
            items: [
                PartialItem(
                    name: "valkey-erp",
                    reason: "readiness failed (PING timeout)",
                    action: StateAction(title: "Open", mutates: false) {}
                ),
                PartialItem(
                    name: "pg17-test",
                    reason: "stopped",
                    action: StateAction(title: "Start", mutates: true) {}
                ),
                PartialItem(name: "pg18-dev", reason: "waiting for probe"),
                PartialItem(name: "mysql-erp", reason: "starting"),
                PartialItem(name: "clickhouse-erp", reason: "starting"),
                PartialItem(name: "traefik-erp", reason: "waiting for probe"),
                PartialItem(
                    name: "mysql-legacy",
                    reason: "stopped",
                    action: StateAction(title: "Start", mutates: true) {}
                ),
            ]
        )
    }

    /// One instance row of the sample content.
    private struct SampleInstance: Identifiable {
        let name: String
        let endpoint: String

        var id: String { name }
    }

    /// The sample content of a loaded state: a grouped list of instances.
    private struct SampleInstanceList: View {
        let instances: [SampleInstance]

        var body: some View {
            VStack(spacing: 0) {
                ForEach(instances) { instance in
                    HStack(spacing: Spacing.s12) {
                        Text(instance.name)
                            .font(.body.weight(.semibold))
                        Spacer(minLength: Spacing.s8)
                        Text(instance.endpoint)
                            .stacklyCode()
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                    .padding(Spacing.rowInsets)
                    if instance.id != instances.last?.id {
                        Divider()
                    }
                }
            }
            .cardSurface()
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s12)
        }
    }

    /// A `StateView` framed like a page, so each shape shows at its real size.
    private struct StateFrame: View {
        let title: String
        let note: String
        let height: CGFloat
        let state: ViewState<[SampleInstance]>

        var body: some View {
            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text(title)
                    .font(.body.weight(.semibold))
                Text(note)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                StateView(state: state, retry: {}, openDiagnostics: {}) { instances in
                    SampleInstanceList(instances: instances)
                }
                .frame(height: height)
                .background(StacklyColor.surfaceWindow)
                .clipShape(RoundedRectangle(cornerRadius: Radius.medium.value))
                .overlay {
                    RoundedRectangle(cornerRadius: Radius.medium.value)
                        .strokeBorder(Color(nsColor: .separatorColor))
                }
            }
        }
    }

    // MARK: - Controls

    private struct ControlsSection: View {
        @Binding var isDaemonUnreachable: Bool
        @Binding var isRefreshing: Bool

        var body: some View {
            GallerySection(
                "Controls",
                note: "Daemon Unreachable sets MutationAvailability to blocked for this page: "
                    + "actions that change data turn disabled with the reason as help, while "
                    + "Retry, Open Diagnostics, Browse Catalog, and Copy stay enabled. "
                    + "Refreshing disables Retry in the stale banners and shows its spinner."
            ) {
                HStack(spacing: Spacing.s22) {
                    Toggle("Daemon Unreachable", isOn: $isDaemonUnreachable)
                    Toggle("Refreshing", isOn: $isRefreshing)
                }
                .toggleStyle(.checkbox)
            }
        }
    }

    // MARK: - Shapes

    private struct ShapesSection: View {
        let isRefreshing: Bool

        var body: some View {
            GallerySection(
                "Every Shape",
                note: "Each frame is a StateView of a ViewState from ViewState.resolve. The "
                    + "order is fixed: unsupported, failed (no data yet), loading, empty, loaded. "
                    + "Stale content is never dimmed."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s22) {
                    LoadingFrame()
                    StateFrame(
                        title: "Empty · no data",
                        note: "The data loaded and holds nothing. New Instance… mutates, Browse "
                            + "Catalog does not.",
                        height: 340,
                        state: .resolve(
                            value: [],
                            error: nil,
                            lastLoadedAt: StatesSample.lastKnown,
                            emptyContent: StatesSample.noData
                        )
                    )
                    StateFrame(
                        title: "Empty · no results",
                        note: "The filter left nothing; the data is kept.",
                        height: 300,
                        state: .resolve(
                            value: StatesSample.instances,
                            error: nil,
                            lastLoadedAt: StatesSample.lastKnown,
                            emptyContent: StatesSample.noResults
                        )
                    )
                    StateFrame(
                        title: "Failed",
                        note: "The first load failed: the daemon's message and code, Retry, and "
                            + "Open Diagnostics. VoiceOver announces it once.",
                        height: 340,
                        state: .resolve(
                            value: nil,
                            error: StatesSample.loadFailure,
                            lastLoadedAt: nil,
                            emptyContent: StatesSample.noData
                        )
                    )
                    StateFrame(
                        title: "Unsupported",
                        note: "The capability is not supported: the reason, and no action for it.",
                        height: 320,
                        state: .resolve(
                            unsupported: StatesSample.backupsUnsupported,
                            value: StatesSample.instances,
                            error: nil,
                            lastLoadedAt: StatesSample.lastKnown,
                            emptyContent: StatesSample.noData
                        )
                    )
                    StateFrame(
                        title: "Loaded",
                        note: "The data loaded.",
                        height: 200,
                        state: .resolve(
                            value: StatesSample.instances,
                            error: nil,
                            lastLoadedAt: StatesSample.lastKnown,
                            emptyContent: StatesSample.noData
                        )
                    )
                    StateFrame(
                        title: "Loaded · stale, refresh failed",
                        note: "A refresh failed after data loaded: the data stays, with a caution "
                            + "banner and Retry.",
                        height: 340,
                        state: .resolve(
                            value: StatesSample.instances,
                            error: FailureContent(
                                title: "Couldn’t Load Instances",
                                message: "the request timed out after 10 seconds"
                            ),
                            lastLoadedAt: StatesSample.lastKnown,
                            isRefreshing: isRefreshing,
                            emptyContent: StatesSample.noData
                        )
                    )
                    StateFrame(
                        title: "Empty · stale, outdated",
                        note: "The source reports its data as out of date. Empty shows the banner "
                            + "too.",
                        height: 460,
                        state: .resolve(
                            value: [],
                            error: nil,
                            lastLoadedAt: StatesSample.lastKnown,
                            isRefreshing: isRefreshing,
                            emptyContent: StatesSample.noData,
                            outdated: (since: StatesSample.catalogUpdated, source: "Catalog")
                        )
                    )
                    StateFrame(
                        title: "Loaded · partial",
                        note: "Five items show, then “and 2 more” with Show All. Start mutates.",
                        height: 460,
                        state: .resolve(
                            value: StatesSample.instances,
                            error: nil,
                            lastLoadedAt: StatesSample.lastKnown,
                            emptyContent: StatesSample.noData,
                            partial: StatesSample.partial
                        )
                    )
                }
            }
        }
    }

    /// The loading shape, with a button that enters loading again to show the 300 ms delay
    /// start over.
    private struct LoadingFrame: View {
        @State private var entry = 0

        var body: some View {
            VStack(alignment: .leading, spacing: Spacing.s6) {
                StateFrame(
                    title: "Loading",
                    note: "The spinner appears only after 300 ms. Enter Loading Again starts the "
                        + "delay over.",
                    height: 120,
                    state: .loading
                )
                .id(entry)
                Button("Enter Loading Again") { entry += 1 }
            }
        }
    }

    // MARK: - Live

    private struct LiveSection: View {
        private enum Step: String, CaseIterable, Identifiable {
            case loading = "Loading"
            case empty = "Empty"
            case failed = "Failed"
            case loaded = "Loaded"

            var id: Self { self }
        }

        @State private var step = Step.loading

        var body: some View {
            GallerySection(
                "Shape Changes",
                note: "Shape changes crossfade in 0.2 s, also under Reduce Motion. Entering "
                    + "Loading starts the 300 ms delay over."
            ) {
                Picker("Shape", selection: $step) {
                    ForEach(Step.allCases) { step in
                        Text(step.rawValue).tag(step)
                    }
                }
                .pickerStyle(.segmented)
                .fixedSize()
                StateView(state: state, retry: { step = .loading }, openDiagnostics: {}) {
                    instances in
                    SampleInstanceList(instances: instances)
                }
                .frame(height: 340)
                .background(StacklyColor.surfaceWindow)
                .clipShape(RoundedRectangle(cornerRadius: Radius.medium.value))
                .overlay {
                    RoundedRectangle(cornerRadius: Radius.medium.value)
                        .strokeBorder(Color(nsColor: .separatorColor))
                }
            }
        }

        private var state: ViewState<[SampleInstance]> {
            let value: [SampleInstance]? =
                switch step {
                case .loading, .failed: nil
                case .empty: []
                case .loaded: StatesSample.instances
                }
            return .resolve(
                value: value,
                error: step == .failed ? StatesSample.loadFailure : nil,
                lastLoadedAt: value == nil ? nil : StatesSample.lastKnown,
                emptyContent: StatesSample.noData
            )
        }
    }

    // MARK: - Placeholders

    private struct PlaceholderSection: View {
        var body: some View {
            GallerySection(
                "Placeholders",
                note: "StatePlaceholder comes in page size (title2, 44 pt symbol, 80 pt padding) "
                    + "and section size inside a card (title3, 34 pt symbol, 34 pt padding). "
                    + "InlineEmptyRow is one sentence inside a grouped list."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s12) {
                    StatePlaceholder(
                        .noData(
                            symbol: "clock.arrow.circlepath",
                            title: "No Backups Yet",
                            message: "Backups are checksummed. Restoring into a new instance "
                                + "leaves pg17-dev untouched.",
                            primaryAction: StateAction(title: "Back Up Now", mutates: true) {}
                        ),
                        size: .section
                    )
                    .cardSurface()
                    StatePlaceholder(StatesSample.backupsUnsupported, size: .section)
                        .cardSurface()
                    VStack(alignment: .leading, spacing: Spacing.s8) {
                        Text("Operations")
                            .font(.headline)
                            .padding(.horizontal, Spacing.s4)
                        InlineEmptyRow("Nothing running.")
                            .cardSurface()
                    }
                }
            }
        }
    }

    // MARK: - Banners

    private struct BannerSection: View {
        var body: some View {
            GallerySection(
                "Banners",
                note: "StateBanner per tone: the tone symbol always shows, the body is callout "
                    + "textSecondary, and the tint is opaque. A critical banner is announced "
                    + "once. The last one sits on the window base, like the shell's banner."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s12) {
                    StateBanner(
                        tone: .critical,
                        title: "Couldn’t start: port in use",
                        message: "Stackly never moves a fixed port silently. Pick another port or "
                            + "stop the process holding it.",
                        code: "E_PORT_IN_USE · Bind failed: 127.0.0.1:3307 is in use by mysqld "
                            + "(PID 812), not owned by Stackly. The fixed port was not changed.",
                        actions: [
                            StateAction(title: "Use Port 3308", mutates: true) {},
                            StateAction(title: "Show Logs", mutates: false) {},
                        ]
                    )
                    StateBanner(
                        tone: .caution,
                        title: "The config file changed outside Stackly.",
                        message: "my.cnf no longer matches revision 4. Review the difference "
                            + "before the next restart.",
                        actions: [StateAction(title: "Review Changes", mutates: false) {}]
                    )
                    StateBanner(
                        tone: .positive,
                        title: "Backup verified.",
                        message: "312 MB · sha256 verified · restore tested"
                    )
                    StateBanner(
                        tone: .neutral,
                        title: "Stopped by you.",
                        message: "Data is kept. Start the instance to use it again."
                    )
                    StateBanner(
                        tone: .informative,
                        title: "Installing PostgreSQL 18.0",
                        message: "Downloading and verifying the runtime."
                    )
                    StateBanner(
                        tone: .caution,
                        title: "Can’t connect to stacklyd.",
                        message: "Showing the last known state from 14:02. Actions are disabled "
                            + "until the daemon is reachable.",
                        actions: [StateAction(title: "Reconnect", mutates: false) {}],
                        base: .window
                    )
                }
            }
        }
    }

    // MARK: - Gate

    private struct GateSection: View {
        var body: some View {
            GallerySection(
                "Mutation Gate and Unsupported",
                note: "Start uses .mutationGated(); Copy Endpoint does not change data and is "
                    + "never gated. Back Up Now uses .disabled(reason:) with the reason shown as "
                    + "DisabledReasonText, never only as a tooltip."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s12) {
                    HStack(spacing: Spacing.s8) {
                        Button("Start") {}
                            .mutationGated()
                        Button("Copy Endpoint") {}
                    }
                    VStack(alignment: .leading, spacing: Spacing.s6) {
                        Button("Back Up Now") {}
                            .disabled(reason: StatesSample.backupReason)
                        DisabledReasonText(StatesSample.backupReason)
                    }
                }
            }
        }
    }

    #Preview("Light") {
        StatesGalleryPage().galleryPreview(.light)
    }

    #Preview("Dark") {
        StatesGalleryPage().galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        StatesGalleryPage().galleryPreview(.narrowWidth)
    }
#endif
