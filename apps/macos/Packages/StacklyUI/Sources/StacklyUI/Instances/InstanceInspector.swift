import StacklyClient
import SwiftUI

/// The inspector of the Instances page for one instance (spec 0022, *Inspector*): the
/// header with the main action, the banner of a problem the person can fix, and the facts.
///
/// It builds only from the ``InstanceRowModel`` and the main action it is given, so a
/// preview or a snapshot shows it without a store.
struct InstanceInspector: View {
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    let row: InstanceRowModel
    let action: StacklyAction
    let isPrimary: Bool
    let open: StacklyAction
    let model: InstanceDetailModel
    let secrets: SecretsClient

    var body: some View {
        let summary = row.summary
        InspectorHeader(
            title: summary.name, subtitle: summary.displayName, tile: row.tile,
            status: row.appearance
        ) {
            Group {
                if isPrimary {
                    ActionButton(action, prominence: .primary, showsReason: true)
                } else {
                    ActionButton(action, prominence: .secondary, showsReason: true)
                }
            }
            .help(
                CommandMap.helpText(
                    for: InstanceDetailPage.command(of: action), title: action.title))
            ActionButton(open, prominence: .secondary, showsReason: false)
                .help(CommandMap.helpText(for: .openInstance, title: open.title))
        }
        banner
        CardSection {
            provider
            LabeledContent("Version") { Text(row.version ?? "—") }
            endpoints
            LabeledContent("Ownership") { Text(Self.ownershipText(summary.ownership)) }
            LabeledContent("Readiness") { Text(ReadinessText.text(for: summary)) }
            LabeledContent("Health") { Text(Self.healthText(summary.health)) }
            process
            if let restarts = Self.restartsText(summary.restart) {
                LabeledContent("Restarts") { Text(restarts) }
            }
            LabeledContent("Desired") { Text(Self.desiredText(summary.desired)) }
            if summary.logsUnavailable {
                LabeledContent("Logs") { Text("Unavailable") }
            }
            dataRow
            connectionRow
            idRow
        }
        .labeledContentStyle(.stackly)
        .accessibilityIdentifier(AccessibilityID.Instances.inspector)
        .task(id: row.id) { await model.openForInspector() }
        .onChange(of: summary, initial: true) { _, new in
            model.observe(new, announcesReady: false)
        }
    }

    // MARK: - Banner

    @ViewBuilder private var banner: some View {
        if let item = AttentionRules.banner(
            for: row.summary, now: fixtureNow ?? Date(), locale: locale, timeZone: timeZone)
        {
            StateBanner(
                tone: item.tone, title: item.title, message: item.message,
                code: item.rule == "I1" ? Self.excerpt(row.summary.lastFailure) : nil)
        }
    }

    /// The lines of the output of the failed process, one per line, or `nil` when empty.
    static func excerpt(_ failure: InstanceLastFailure?) -> String? {
        let lines = (failure?.excerpt ?? []).flatMap {
            $0.split(whereSeparator: \.isNewline).map(String.init)
        }
        return lines.isEmpty ? nil : lines.joined(separator: "\n")
    }

    // MARK: - Rows

    private var provider: some View {
        LabeledContent("Provider") {
            VStack(alignment: .trailing, spacing: Spacing.s2) {
                Text(row.providerTitle)
                Text("\(row.summary.provider.name) \(row.summary.provider.revision)")
                    .font(.footnote)
            }
        }
    }

    private var endpoints: some View {
        LabeledContent("Endpoints") {
            let endpoints = row.summary.endpoints ?? []
            if endpoints.isEmpty {
                Text("—")
            } else {
                VStack(alignment: .trailing, spacing: Spacing.s2) {
                    ForEach(Array(endpoints.enumerated()), id: \.offset) { _, endpoint in
                        Text(InstanceText.endpointLine(endpoint))
                            .font(.callout.monospaced())
                    }
                }
            }
        }
    }

    private var process: some View {
        LabeledContent("Process") {
            if let process = row.summary.process {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text(
                        Self.processText(
                            process, now: fixtureNow ?? context.date))
                }
            } else {
                Text("Not running")
            }
        }
    }

    /// The size the daemon already measured, or "—", and the folder on the line under it.
    private var dataRow: some View {
        LabeledContent("Data") {
            VStack(alignment: .trailing, spacing: Spacing.s2) {
                Text(DetailFormat.dataSize(.loaded(model.usage)))
                if let path = model.detail?.dataPath {
                    Text(DetailFormat.dataPath(path, home: NSHomeDirectory()))
                        .font(.footnote.monospaced())
                        .help(path)
                }
            }
        }
    }

    /// The first connection string, masked, with Show Password and Copy, or the reason there
    /// is none to copy.
    private var connectionRow: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            Text("Connection")
            if let detail = model.detail {
                if let text = DetailFormat.connectionUnavailableText(detail) {
                    reason(text)
                } else if let first = detail.connections.first {
                    ConnectionEntryRow(connection: first, detail: detail, secrets: secrets)
                } else {
                    reason(DetailFormat.noConnectionText)
                }
            } else {
                Text("—").foregroundStyle(StacklyColor.textSecondary)
            }
        }
    }

    private func reason(_ text: String) -> some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(StacklyColor.textSecondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var idRow: some View {
        let id = row.summary.id.uuidString.lowercased()
        return HStack(spacing: Spacing.s8) {
            LabeledContent("ID") {
                Text(id).font(.callout.monospaced())
            }
            CopyButton(
                id, what: "instance ID", style: .icon, announcement: "ID copied.", key: "instanceId"
            )
        }
    }

    // MARK: - Words

    /// "PID 4001 · up 3m04s", with " · adopted" for a process the daemon found after its own
    /// restart.
    static func processText(_ process: InstanceProcess, now: Date) -> String {
        let up = UptimeText.text(since: process.startedAt, now: now)
        return "PID \(process.pid) · up \(up)" + (process.adopted ? " · adopted" : "")
    }

    /// "Manual", or "Unknown" for a value this build does not know.
    static func ownershipText(_ ownership: InstanceOwnership?) -> String {
        switch ownership ?? .manual {
        case .manual: "Manual"
        case .unrecognized: "Unknown"
        }
    }

    /// "Healthy", "Degraded", "Unhealthy", or "Unknown", with the detail of the daemon.
    static func healthText(_ health: InstanceHealth) -> String {
        let word: String =
            switch health.state {
            case .healthy: "Healthy"
            case .degraded: "Degraded"
            case .unhealthy: "Unhealthy"
            case .unknown: "Unknown"
            }
        return health.detail.map { "\(word): \($0)" } ?? word
    }

    /// "2 in the last 1 min", "Stopped restarting", or `nil` when nothing restarted.
    static func restartsText(_ restart: InstanceRestart) -> String? {
        if restart.exhausted { return "Stopped restarting" }
        guard restart.attempts > 0 else { return nil }
        return "\(restart.attempts) in the last \(restart.windowSeconds / 60) min"
    }

    /// "Running" or "Stopped".
    static func desiredText(_ desired: InstanceDesired) -> String {
        switch desired {
        case .running: "Running"
        case .stopped: "Stopped"
        }
    }
}
