import StacklyClient
import SwiftUI

/// The Metrics tab of the detail page (spec 0028, *Layar Metrics*, AC-13): the scope control
/// and the health line, five cards, the CPU graph, and the process list; or "No Live Metrics"
/// while the instance is not running. The tab reads when it appears and stops when it goes
/// away; the history stays in the model, which the page's registry keeps per instance.
struct InstanceMetricsTab: View {
    @Environment(\.loadingDelay) private var loadingDelay
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.timeZone) private var timeZone
    @Environment(\.locale) private var locale
    @State private var width: CGFloat = 0

    /// The tab's model.
    let model: InstanceMetricsModel

    /// The name of the instance, for "{name} isn’t running."
    let instanceName: String

    /// The main action of the instance; Start is offered on the "No Live Metrics" state only
    /// when it can start the instance (spec 0028, AC-13).
    let action: StacklyAction

    /// How many snapshot rounds the shell completed; a change after a reconnect resumes the
    /// polling.
    let completedRounds: Int

    var body: some View {
        content
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .onGeometryChange(for: CGFloat.self) {
                $0.size.width
            } action: {
                width = $0
            }
            .onAppear { model.open() }
            .onDisappear { model.close() }
            .onChange(of: completedRounds) { model.reconnected() }
    }

    // MARK: - Content

    /// What the content area shows.
    enum Display: Equatable {
        case loading
        case waiting
        case failed(ErrorBody)
        case notRunning
        case live
    }

    /// What the content area of `model` shows: the live view whenever the daemon says the
    /// instance is running, else the state of the tab.
    static func display(of model: InstanceMetricsModel) -> Display {
        guard let reply = model.reply else {
            if model.isOffline { return .waiting }
            if case .failed(let body) = model.phase { return .failed(body) }
            return .loading
        }
        return reply.state == .live ? .live : .notRunning
    }

    @ViewBuilder private var content: some View {
        switch Self.display(of: model) {
        case .loading:
            DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
                .frame(maxWidth: .infinity, minHeight: 240)
        case .waiting:
            StatePlaceholder(
                .noData(
                    symbol: "bolt.horizontal.circle", title: "Waiting for stacklyd",
                    message: "Metrics show here once the daemon is reachable."),
                size: .section)
        case .failed(let body):
            StatePlaceholder(
                FailureContent(
                    title: "Couldn’t Load Metrics", message: body.message, code: body.code),
                size: .section,
                retry: { model.retry() },
                openDiagnostics: { navigator?.show(.diagnostics) })
        case .notRunning:
            notRunning
        case .live:
            live
        }
    }

    // MARK: - Not running

    private var notRunning: some View {
        VStack(spacing: Spacing.s12) {
            StatePlaceholder(
                .noData(
                    symbol: "waveform.path.ecg", title: "No Live Metrics",
                    message:
                        "\(instanceName) isn’t running. Start it to collect CPU, memory and readiness data."
                ),
                size: .section)
            if action.id != InstanceVerb.stop.rawValue {
                ActionButton(action, prominence: .primary, showsReason: true)
            }
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: - Live

    @ViewBuilder private var live: some View {
        if let reply = model.reply {
            VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                if let failure = model.refreshFailure {
                    StateBanner(
                        tone: .caution, title: "Couldn’t refresh metrics",
                        message: failure.message, code: failure.code)
                }
                header(reply)
                cards(reply)
                MetricsGraph(
                    samples: model.samples, scope: model.scope, capacity: reply.capacity,
                    intervalSeconds: reply.intervalSeconds)
                MetricsProcessList(
                    lines: MetricsFormat.processLines(model.processes),
                    truncated: model.processesTruncated,
                    treeCount: model.latestSample?.tree.processCount)
            }
        }
    }

    /// The columns of the cards for a content width: all five in a row from 760 pt, three from
    /// 480 pt, else two.
    static func cardColumns(for width: CGFloat) -> Int {
        if width >= 760 { return 5 }
        return width >= 480 ? 3 : 2
    }

    private func header(_ reply: MetricsReply) -> some View {
        let health = MetricsFormat.healthLine(reply, timeZone: timeZone, locale: locale)
        return ViewThatFits(in: .horizontal) {
            HStack(spacing: Spacing.s12) {
                scopePicker
                Spacer(minLength: Spacing.s8)
                healthLine(health)
            }
            VStack(alignment: .leading, spacing: Spacing.s8) {
                scopePicker
                healthLine(health)
            }
        }
    }

    private var scopePicker: some View {
        Picker(
            "Scope",
            selection: Binding(get: { model.scope }, set: { model.scope = $0 })
        ) {
            ForEach(InstanceMetricsModel.Scope.allCases) { scope in
                Text(scope.title).tag(scope)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
    }

    private func healthLine(_ health: MetricsFormat.HealthLine) -> some View {
        HStack(spacing: Spacing.s6) {
            Circle()
                .fill(health.tone.fill)
                .frame(width: ScaledSize.statusDot.base, height: ScaledSize.statusDot.base)
            Text(health.text)
                .foregroundStyle(StacklyColor.textSecondary)
                .stacklyNumeric()
        }
    }

    private func cards(_ reply: MetricsReply) -> some View {
        let cards = MetricsFormat.cards(
            usage: model.latestUsage, reply: reply, timeZone: timeZone, locale: locale)
        let columns = Array(
            repeating: GridItem(.flexible(), spacing: Spacing.s12, alignment: .top),
            count: Self.cardColumns(for: width))
        return LazyVGrid(columns: columns, alignment: .leading, spacing: Spacing.s12) {
            ForEach(cards) { MetricCard(card: $0) }
        }
    }
}

/// One of the five cards: the title, the value or "Unavailable", the lines under it, and the
/// scope or the note at the bottom.
private struct MetricCard: View {
    let card: MetricsFormat.Card

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            Text(card.title)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
            Text(card.headline)
                .font(.title3.weight(.semibold))
                .foregroundStyle(
                    card.isUnavailable
                        ? AnyShapeStyle(StacklyColor.textSecondary) : AnyShapeStyle(.primary)
                )
                .stacklyNumeric()
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            ForEach(card.lines, id: \.self) { line in
                Text(line)
                    .font(.footnote)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .stacklyNumeric()
            }
            if let subtitle = card.subtitle {
                Text(subtitle)
                    .font(.footnote)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
        }
        .padding(Spacing.cardInsets)
        .frame(maxWidth: .infinity, minHeight: 88, alignment: .topLeading)
        .cardSurface()
    }
}
