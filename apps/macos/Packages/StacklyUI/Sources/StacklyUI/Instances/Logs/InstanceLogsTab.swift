import StacklyClient
import SwiftUI

/// The Logs tab of the detail page (spec 0027, *Layar Logs*, AC-11): All, stdout, and stderr,
/// the filter, Follow, Load Earlier, Copy Shown Lines, the log, and the retention footer.
/// The tab reads when it appears and stops following when it goes away; the rows stay in
/// the model, which the page's registry keeps per instance.
struct InstanceLogsTab: View {
    @Environment(\.loadingDelay) private var loadingDelay
    @Environment(\.timeZone) private var timeZone
    @Environment(\.locale) private var locale
    @State private var query: String

    /// The tab's model.
    let model: InstanceLogsModel

    /// How many snapshot rounds the shell completed; a change after a reconnect resumes the
    /// follow.
    let completedRounds: Int

    /// Creates the tab of one instance.
    /// - Parameters:
    ///   - model: The tab's model, from ``InstanceDetailModels``.
    ///   - completedRounds: The shell's completed rounds.
    init(model: InstanceLogsModel, completedRounds: Int) {
        self.model = model
        _query = State(initialValue: model.query)
        self.completedRounds = completedRounds
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            toolbar
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            Text(Self.footer(newest: model.newestSeq, retention: model.retention))
                .font(.caption)
                .foregroundStyle(StacklyColor.textSecondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onAppear {
            model.timeZone = timeZone
            model.locale = locale
            model.open()
        }
        .onChange(of: timeZone) { model.timeZone = timeZone }
        .onChange(of: locale) { model.locale = locale }
        .onDisappear { model.close() }
        .onChange(of: completedRounds) { model.reconnected() }
        .onChange(of: query) { _, new in model.setQuery(new) }
    }

    // MARK: - Toolbar

    private var toolbar: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: Spacing.s12) {
                filters
                Spacer(minLength: Spacing.s8)
                controls
            }
            VStack(alignment: .leading, spacing: Spacing.s8) {
                filters
                HStack(spacing: Spacing.s12) { controls }
            }
        }
    }

    private var filters: some View {
        HStack(spacing: Spacing.s12) {
            Picker(
                "Source",
                selection: Binding(get: { model.source }, set: { model.setSource($0) })
            ) {
                ForEach(InstanceLogsModel.Source.allCases) { source in
                    Text(source.title).tag(source)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            TextField("Filter", text: $query)
                .textFieldStyle(.roundedBorder)
                .frame(width: 220)
        }
    }

    @ViewBuilder private var controls: some View {
        Toggle("Follow", isOn: Binding(get: { model.follow }, set: { model.setFollow($0) }))
            .toggleStyle(.switch)
            .controlSize(.small)
            .help(model.followFailure ?? "Show new lines as they arrive")
        Button("Load Earlier") { model.loadEarlier() }
            .buttonStyle(.bordered)
            .controlSize(.small)
            .disabled(model.earlier == nil || model.isLoadingEarlier)
        CopyButton(title: "Copy Shown Lines", what: "shown lines") { model.shownText }
            .disabled(model.rows.isEmpty)
    }

    // MARK: - Content

    @ViewBuilder private var content: some View {
        switch Self.display(of: model) {
        case .loading:
            DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .lines:
            LogView(
                model.lines,
                follow: Binding(get: { model.follow }, set: { model.setFollow($0) }),
                sourceTemplate: LogLineMapper.stacklySource,
                copyText: { model.copyText(of: $0) })
        case .placeholder(let empty):
            StatePlaceholder(empty, size: .section)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background { Color.clear.consoleSurface() }
                .clipShape(RoundedRectangle(cornerRadius: Radius.medium.value))
        }
    }

    /// What the content area shows.
    enum Display {
        case loading
        case lines
        case placeholder(EmptyContent)
    }

    /// What the content area of `model` shows: the lines whenever some are loaded, else the
    /// state of the tab (spec 0027, *State*).
    static func display(of model: InstanceLogsModel) -> Display {
        if model.rows.isEmpty == false { return .lines }
        switch model.phase {
        case .loading:
            return .loading
        case .unreadable(let reason):
            return .placeholder(
                .noData(
                    symbol: Tone.critical.symbol, title: "Logs can’t be read", message: reason,
                    primaryAction: StateAction(
                        title: "Try Again", mutates: false, perform: { model.retry() })))
        case .ready:
            if model.isOffline {
                return .placeholder(
                    .noData(
                        symbol: "bolt.horizontal.circle", title: "Waiting for stacklyd",
                        message: "Lines show here once the daemon is reachable."))
            }
            if model.isFiltered {
                if model.scanLimited {
                    return .placeholder(
                        .noData(
                            symbol: "magnifyingglass",
                            title: "No matches in the last 8 MB searched.",
                            message: "Load Earlier searches further back."))
                }
                return .placeholder(
                    .noData(
                        symbol: "magnifyingglass", title: "No lines match this filter.",
                        message: "Try another source or filter text."))
            }
            return .placeholder(
                .noData(
                    symbol: "text.alignleft", title: "No output yet",
                    message: "Lines appear here when the instance writes to stdout or stderr."))
        }
    }

    /// `Cursor <seq> · Retained 5 files × 20 MiB per stream · Secrets are redacted before
    /// storage`, with the numbers of the store.
    static func footer(newest: UInt64?, retention: LogRetention?) -> String {
        let cursor = newest.map { "Cursor \($0)" } ?? "Cursor none"
        let kept = retention ?? LogRetention(segmentsPerStream: 5, segmentMaxBytes: 20 << 20)
        let size = LogLineMapper.byteText(kept.segmentMaxBytes).replacingOccurrences(
            of: ".0 ", with: " ")
        return
            "\(cursor) · Retained \(kept.segmentsPerStream) files × \(size) per stream · Secrets are redacted before storage"
    }
}
