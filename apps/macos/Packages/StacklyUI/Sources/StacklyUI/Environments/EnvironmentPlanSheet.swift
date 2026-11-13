import StacklyClient
import SwiftUI

/// The sheet "Plan for {name}" (spec 0044, *Layar*, AC-20): one row per service in start
/// order with its verb (Create, Start, Keep), the Install line of a create that needs a new
/// runtime, the providers that need approval, the differences, the secrets each create uses,
/// the blockers above the list, and the Problems of a manifest that is not valid. It only
/// reads: from a detail page it has Done; from Import… it has Cancel and Import.
struct EnvironmentPlanSheet: View {
    let presenter: EnvironmentPlanPresenter
    let catalog: CatalogStore?
    /// The instances, for the version of a kept instance.
    let instances: InstanceStore?
    let home: String
    /// Opens the detail of a provider in the Catalog (spec 0037).
    let reviewProvider: @MainActor (String) -> Void

    var body: some View {
        StacklySheet(
            title: EnvironmentCopy.planTitle(presenter.name),
            subtitle: EnvironmentCopy.planSubtitle,
            primary: primary,
            cancel: presenter.isImport
                ? StacklyAction(
                    id: "environment.plan.cancel", title: "Cancel", mutates: false,
                    perform: { presenter.close() })
                : nil,
            minWidth: 560,
            showsCancel: presenter.isImport
        ) {
            EnvironmentPlanContent(
                phase: presenter.phase, displayName: displayName,
                version: { [instances] id in instances?.store.items[id]?.runtimeVersion },
                home: home,
                reviewProvider: reviewProvider,
                useThisFile: { Task { await presenter.importPlan(relink: true) } })
        }
    }

    /// Done from a detail page; Import, off with its reason, from Import….
    private var primary: StacklyAction {
        guard presenter.isImport else {
            return StacklyAction(
                id: "environment.plan.done", title: "Done", mutates: false,
                perform: { presenter.close() })
        }
        var reason: String?
        var busy = false
        switch presenter.phase {
        case .plan(let plan, _), .importFailed(let plan, _, _):
            reason = EnvironmentPlanPresenter.importBlockedReason(plan)
        case .importing:
            busy = true
        case .loading:
            busy = true
            reason = "The plan is still loading."
        case .failed:
            reason = "The plan could not be read."
        case .idle, .choosing:
            reason = nil
        }
        return StacklyAction(
            id: "environment.plan.import", title: "Import", mutates: true,
            disabledReason: reason, isBusy: busy,
            perform: { Task { await presenter.importPlan() } })
    }

    /// The software name of a provider, from the catalog, or the provider itself.
    private var displayName: (String) -> String {
        let providers = catalog?.providers ?? []
        return { provider in
            providers.first { $0.provider == provider }?.displayName ?? provider
        }
    }
}

/// The body of the plan sheet for one phase.
struct EnvironmentPlanContent: View {
    let phase: EnvironmentPlanPresenter.Phase
    let displayName: (String) -> String
    /// The runtime version of an existing instance, for a kept one.
    let version: (UUID) -> String?
    let home: String
    let reviewProvider: @MainActor (String) -> Void
    let useThisFile: @MainActor () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s16) {
            switch phase {
            case .idle, .choosing, .loading:
                HStack {
                    Spacer()
                    DelayedProgress()
                    Spacer()
                }
                .frame(minHeight: 120)
            case .failed(let error, _):
                StateBanner(
                    tone: .critical, title: "Couldn’t Read the Plan",
                    message: DaemonText.sentence(error.message), code: error.code)
            case .plan(let plan, _), .importing(let plan, _):
                planBody(plan, importError: nil)
            case .importFailed(let plan, _, let error):
                planBody(plan, importError: error)
            }
        }
    }

    @ViewBuilder
    private func planBody(_ plan: EnvPlan, importError: ErrorBody?) -> some View {
        if let importError {
            StateBanner(
                tone: .critical, title: DaemonText.sentence(importError.message),
                message: importError.hint.map(DaemonText.sentence), code: importError.code)
        }
        if plan.validation.valid == false {
            problems(plan.validation)
        } else {
            if plan.registration == .nameTaken, let name = plan.validation.name {
                StateBanner(
                    tone: .caution,
                    title: EnvironmentCopy.nameTaken(
                        name, location: plan.registeredLocation, home: home),
                    actions: plan.environment == nil
                        ? []
                        : [StateAction(title: "Use This File", mutates: true, perform: useThisFile)]
                )
            }
            let blockers = (plan.blockers ?? []).filter { $0.error.code != "env.name_taken" }
            if blockers.isEmpty == false {
                StateBanner(
                    tone: .critical,
                    title: blockers.count == 1
                        ? "1 problem blocks this plan."
                        : "\(blockers.count) problems block this plan.",
                    items: Self.items(blockers))
            }
            VStack(alignment: .leading, spacing: Spacing.s12) {
                ForEach(Self.ordered(plan), id: \.alias) { item in
                    PlanItemRow(
                        item: item, displayName: displayName,
                        version: item.instance.id.flatMap(version),
                        reviewProvider: reviewProvider)
                }
            }
            Text(EnvironmentCopy.planFootnote)
                .font(.footnote)
                .foregroundStyle(StacklyColor.textSecondary)
        }
    }

    /// The items in start order; an item the order does not name follows in manifest order.
    static func ordered(_ plan: EnvPlan) -> [PlanItem] {
        let items = plan.items ?? []
        let order = plan.order ?? []
        let named = order.compactMap { alias in items.first { $0.alias == alias } }
        return named + items.filter { order.contains($0.alias) == false }
    }

    /// The blockers as banner items, one per blocker (spec 0044, *Layar*): the service, then
    /// the message and the hint. An item is known by its name, so a service with more than
    /// one blocker names the later ones with their error code.
    static func items(_ blockers: [PlanBlocker]) -> [PartialItem] {
        var seen: Set<String> = []
        return blockers.map { blocker in
            let service = blocker.service ?? "Environment"
            let name =
                seen.insert(service).inserted ? service : "\(service) (\(blocker.error.code))"
            let message = DaemonText.sentence(blocker.error.message)
            let reason =
                blocker.error.hint.map { "\(message) \(DaemonText.sentence($0))" } ?? message
            return PartialItem(name: name, reason: reason)
        }
    }

    private func problems(_ validation: EnvValidation) -> some View {
        CardSection("Problems") {
            CheckList(
                (validation.findings ?? []).enumerated().map { index, finding in
                    CheckItem(
                        id: "\(index)-\(finding.code)",
                        status: finding.severity == .error ? .failed : .warning,
                        title: finding.message, detail: finding.location)
                }, layout: .single)
        }
    }
}

/// One service of the plan: the verb badge, the line, and the notes under it.
struct PlanItemRow: View {
    let item: PlanItem
    let displayName: (String) -> String
    /// The runtime version of the existing instance, for a kept one.
    let version: String?
    let reviewProvider: @MainActor (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                ToneBadge(Self.badge(item.action))
                Text(Self.line(item, displayName: displayName, version: version))
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(Self.notes(item, displayName: displayName), id: \.self) { note in
                Text(note)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .padding(.leading, Spacing.s22)
            }
            if item.trust == .approvalRequired || item.trust == .revoked {
                HStack(spacing: Spacing.s8) {
                    Text(EnvironmentCopy.needsApproval)
                        .font(.callout)
                        .foregroundStyle(Tone.caution.text)
                    Button(EnvironmentCopy.reviewInCatalog) { reviewProvider(item.provider) }
                        .buttonStyle(.link)
                }
                .padding(.leading, Spacing.s22)
            }
        }
    }

    /// "+ Create" positive, "▸ Start" informative, "= Keep" neutral.
    static func badge(_ action: PlanAction) -> StatusAppearance {
        switch action {
        case .create:
            StatusAppearance(label: "+ Create", tone: .positive, symbol: Tone.positive.symbol)
        case .start:
            StatusAppearance(label: "▸ Start", tone: .informative, symbol: Tone.informative.symbol)
        case .keep: StatusAppearance(label: "= Keep", tone: .neutral, symbol: Tone.neutral.symbol)
        case .unrecognized:
            StatusAppearance(label: "Unknown", tone: .neutral, symbol: Tone.neutral.symbol)
        }
    }

    /// The text of the row: "{alias} → {instance} · {software} {version} · {host}:{port}" for
    /// a create, "· wait for readiness" for a start, and "· {version}" for a keep.
    static func line(
        _ item: PlanItem, displayName: (String) -> String, version: String? = nil
    ) -> String {
        let head = "\(item.alias) → \(item.instance.name)"
        switch item.action {
        case .create:
            var parts = [head]
            if let runtime = item.runtime {
                parts.append("\(displayName(item.provider)) \(runtime.version)")
            }
            if let endpoint = item.endpoints?.first {
                parts.append("\(endpoint.host):\(EnvironmentCopy.portText(endpoint.port))")
            }
            return parts.joined(separator: " · ")
        case .start:
            return "\(head) · wait for readiness"
        case .keep, .unrecognized:
            guard let version = version ?? item.runtime?.version else { return head }
            return "\(head) · \(version)"
        }
    }

    /// The lines under the row: Installs, the differences, and the secrets.
    static func notes(_ item: PlanItem, displayName: (String) -> String) -> [String] {
        var notes: [String] = []
        if item.action == .create, let runtime = item.runtime, runtime.install {
            notes.append("Installs \(displayName(item.provider)) \(runtime.version)")
        }
        for difference in item.differences ?? [] {
            notes.append(EnvironmentCopy.differs(difference.field))
        }
        for secret in item.secrets ?? [] {
            if let line = EnvironmentCopy.secretLine(secret) { notes.append(line) }
        }
        return notes
    }
}
