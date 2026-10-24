import SwiftUI

/// The Connection section at the top of the Diagnostics page: a card with one row per fact
/// about the connection to stacklyd, then Reconnect or Refresh, and Copy Details.
/// It opens even while the daemon is down. Reconnect shows while there is no connection,
/// disabled with its reason while the app is misconfigured; Refresh shows while there is
/// one. Copy Details copies exactly the rows on screen. Every value wraps, the socket path
/// included, and can be selected.
public struct ConnectionDiagnosticsSection: View {
    /// Why Reconnect is disabled while the app is misconfigured.
    static let reconnectDisabledReason = "Relaunch Stackly after fixing the problem."

    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    private let diagnostics: ConnectionDiagnostics
    private let isReconnecting: Bool
    private let reconnect: @MainActor () -> Void
    private let refresh: @MainActor () -> Void

    /// Creates the section.
    /// - Parameters:
    ///   - diagnostics: What to show.
    ///   - isReconnecting: Whether a connection attempt runs now: Reconnect is disabled and
    ///     shows a small spinner.
    ///   - reconnect: Tries to connect now.
    ///   - refresh: Asks the connected daemon for its info and health again.
    public init(
        _ diagnostics: ConnectionDiagnostics,
        isReconnecting: Bool,
        reconnect: @escaping @MainActor () -> Void,
        refresh: @escaping @MainActor () -> Void
    ) {
        self.diagnostics = diagnostics
        self.isReconnecting = isReconnecting
        self.reconnect = reconnect
        self.refresh = refresh
    }

    /// The section's content. The hint redraws every second while the agent's banner row
    /// still counts to its 15th second, like the banner.
    public var body: some View {
        let ticks = DaemonBanner.ticksEverySecond(
            diagnostics.agentHint, now: fixtureNow ?? Date.now)
        if ticks {
            TimelineView(.periodic(from: .now, by: 1)) { context in
                content(now: fixtureNow ?? context.date)
            }
        } else {
            TimelineView(.everyMinute) { context in
                content(now: fixtureNow ?? context.date)
            }
        }
    }

    @ViewBuilder private func content(now: Date) -> some View {
        let rows = diagnostics.rows(
            calendar: calendar, locale: locale, timeZone: timeZone, now: now)
        let presentation = ShellPresentation(status: diagnostics.status)
        VStack(alignment: .leading, spacing: Spacing.s8) {
            Text("Connection")
                .font(.headline)

            DiagnosticsCard(rows: rows)
            HStack(spacing: Spacing.s8) {
                if presentation.hasConnection {
                    Button("Refresh", action: refresh)
                } else {
                    reconnectButton(disabledReason: reconnectDisabledReason(presentation))
                }
                Button("Copy Details") {
                    Self.copy(ConnectionDiagnostics.copyText(rows))
                }
            }
            .buttonStyle(.bordered)
            .padding(.top, Spacing.s4)
            if let reason = reconnectDisabledReason(presentation),
                presentation.hasConnection == false
            {
                DisabledReasonText(reason)
            }
        }
    }

    private func reconnectDisabledReason(_ presentation: ShellPresentation) -> String? {
        presentation.isMisconfigured ? Self.reconnectDisabledReason : nil
    }

    private func reconnectButton(disabledReason: String?) -> some View {
        HStack(spacing: Spacing.s6) {
            Button("Reconnect", action: reconnect)
                .disabled(isReconnecting)
                .disabled(reason: disabledReason)
            if isReconnecting {
                ProgressView()
                    .controlSize(.small)
                    .transition(StacklyMotion.crossfade)

            }
        }
    }

    private static func copy(_ text: String) {
        PlainPasteboard.write(text)
    }
}

/// A card of Diagnostics rows: a label in a fixed column and a value that wraps and can be
/// selected. Every Diagnostics section uses it.
struct DiagnosticsCard: View {
    let rows: [DiagnosticsRow]

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            ForEach(rows) { row in
                LabeledContent(row.label) {
                    DiagnosticsValue(row: row)
                }
            }
        }
        .labeledContentStyle(DiagnosticsRowStyle())
        .textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Spacing.cardInsets)
        .cardSurface()
    }
}

/// The value of a Diagnostics row: the code part monospaced, then the plain part.
private struct DiagnosticsValue: View {
    let row: DiagnosticsRow

    var body: some View {
        Group {
            switch (row.code, row.text) {
            case (let code?, let text?):
                Text("\(Text(code).font(.callout.monospaced())) · \(text)")
            case (let code?, nil):
                Text(code).font(.callout.monospaced())
            case (nil, let text?):
                Text(text)
            case (nil, nil):
                EmptyView()
            }
        }
        .fixedSize(horizontal: false, vertical: true)
    }
}

/// A Diagnostics row: the label in ``StacklyColor/textSecondary`` in a fixed column, and
/// the value filling the rest, wrapping onto more lines when it is long.
private struct DiagnosticsRowStyle: LabeledContentStyle {
    /// The width of the label column, which fits the longest label, "Last connected".
    static let labelWidth: CGFloat = 120

    func makeBody(configuration: Configuration) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            configuration.label
                .foregroundStyle(StacklyColor.textSecondary)
                .frame(width: Self.labelWidth, alignment: .leading)
            configuration.content
                .frame(maxWidth: .infinity, alignment: .leading)
        }

    }
}
