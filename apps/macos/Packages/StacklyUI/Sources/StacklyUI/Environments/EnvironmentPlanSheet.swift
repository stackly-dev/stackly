import StacklyClient
import SwiftUI

/// The sheet "Plan for {name}" (spec 0044, *Layar*, AC-20): one row per service in start
/// order with its verb (Create, Start, Keep), the Install line of a create that needs a new
/// runtime, the providers that need approval, the differences, the secrets each create uses,
/// the blockers above the list, and the Problems of a manifest that is not valid; from spec
/// 0045 the creates whose version comes from the lock, the local overrides that apply, a lock
/// that is out of date, and the warnings of the local override file. From a detail page it
/// has Cancel and Bring Up (spec 0046, AC-19), and a "Needs secret" line has Set…, which opens
/// the secret entry sheet (AC-21); from Import… it has Cancel and Import.
struct EnvironmentPlanSheet: View {
    @Bindable var presenter: EnvironmentPlanPresenter
    let catalog: CatalogStore?
    /// The secret methods behind Set… and the needs of `secret.needs_secret`, or `nil` where
    /// the page has none: then Set… does not show.
    var secrets: SecretsClient?
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
            cancel: StacklyAction(
                id: "environment.plan.cancel", title: "Cancel", mutates: false,
                perform: { presenter.close() }),
            minWidth: 560
        ) {
            EnvironmentPlanContent(
                phase: presenter.phase, displayName: displayName,
                version: { [instances] id in instances?.store.items[id]?.runtimeVersion },
                home: home,
                notice: presenter.notice,
                reviewProvider: reviewProvider,
                useThisFile: { Task { await presenter.importPlan(relink: true) } },
                setSecret: secrets == nil || presenter.isImport
                    ? nil : { presenter.setSecret($0) })
        }
        .sheet(isPresented: secretSheetShown) {
            if let secrets {
                SecretEntrySheet(
                    needs: presenter.secretNeeds ?? [], client: secrets,
                    actionTitle: EnvironmentCopy.reviewPlan,
                    onContinue: { Task { await presenter.secretsEntered(saved: true) } },
                    onCancel: { Task { await presenter.secretsEntered(saved: false) } })
            }
        }
    }

    private var secretSheetShown: Binding<Bool> {
        Binding {
            secrets != nil && presenter.secretNeeds != nil
        } set: { shown in
            if shown == false, presenter.secretNeeds != nil {
                Task { await presenter.secretsEntered(saved: false) }
            }
        }
    }

    /// Bring Up from a detail page, off with its reason; Import, off with its reason, from
    /// Import….
    private var primary: StacklyAction {
        guard presenter.isImport else { return Self.bringUpAction(presenter) }
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
        case .idle, .choosing, .bringingUp, .upFailed:
            reason = nil
        }
        return StacklyAction(
            id: "environment.plan.import", title: "Import", mutates: true,
            disabledReason: reason, isBusy: busy,
            perform: { Task { await presenter.importPlan() } })
    }

    /// Bring Up (spec 0046, AC-19): on only while the plan is `applicable`, busy while
    /// `env.up` is on its way.
    static func bringUpAction(_ presenter: EnvironmentPlanPresenter) -> StacklyAction {
        var reason: String?
        var busy = false
        switch presenter.phase {
        case .plan(let plan, _), .upFailed(let plan, _, _):
            reason = EnvironmentPlanPresenter.upBlockedReason(plan)
        case .bringingUp:
            busy = true
        case .loading, .idle, .choosing:
            busy = true
            reason = "The plan is still loading."
        case .failed:
            reason = "The plan could not be read."
        case .importing, .importFailed:
            reason = nil
        }
        return StacklyAction(
            id: "environment.plan.bringUp", title: EnvironmentCopy.bringUp, mutates: true,
            disabledReason: reason, isBusy: busy,
            perform: { Task { await presenter.bringUp() } })
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
    /// The note above a plan read again after `env.plan_changed`, or `nil`.
    var notice: String?
    let reviewProvider: @MainActor (String) -> Void
    let useThisFile: @MainActor () -> Void
    /// Set… of a "Needs secret" line, or `nil` where the sheet has none.
    var setSecret: (@MainActor (PlanSecret) -> Void)?

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
            case .plan(let plan, _), .importing(let plan, _), .bringingUp(let plan, _):
                planBody(plan, importError: nil)
            case .importFailed(let plan, _, let error), .upFailed(let plan, _, let error):
                planBody(plan, importError: error)
            }
        }
    }

    @ViewBuilder
    private func planBody(_ plan: EnvPlan, importError: ErrorBody?) -> some View {
        if let notice {
            StateBanner(tone: .caution, title: notice)
        }
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
            if let stale = Self.staleServices(plan) {
                StateBanner(tone: .caution, title: EnvironmentCopy.staleLock(stale))
            }
            if let local = plan.local, let warnings = Self.localWarnings(local) {
                StateBanner(
                    tone: .caution,
                    title: EnvironmentCopy.localWarnings(local.file, count: warnings.count),
                    items: warnings)
            }
            let overrides = Self.appliedOverrides(plan)
            VStack(alignment: .leading, spacing: Spacing.s12) {
                ForEach(Self.ordered(plan), id: \.alias) { item in
                    PlanItemRow(
                        item: item, displayName: displayName,
                        version: item.instance.id.flatMap(version),
                        overrides: overrides.filter { $0.service == item.alias },
                        reviewProvider: reviewProvider, setSecret: setSecret)
                }
            }
            Text(EnvironmentCopy.planFootnote)
                .font(.footnote)
                .foregroundStyle(StacklyColor.textSecondary)
        }
    }

    /// The services whose lock entry is out of date, for the caution of a plan that is not
    /// `locked` (spec 0045 *Layar*), or `nil` when there is none. With `locked` each one is a
    /// blocker instead.
    static func staleServices(_ plan: EnvPlan) -> [String]? {
        guard let lock = plan.lock, lock.state == .stale, lock.locked != true else { return nil }
        let services = (lock.stale ?? []).map(\.service)
        return services.isEmpty ? nil : services
    }

    /// The overrides the plan applies to a create. A file with an error finding is not applied
    /// at all (its blocker `env.local_invalid` says so), so then there is none.
    static func appliedOverrides(_ plan: EnvPlan) -> [PlanOverride] {
        guard let local = plan.local, local.state == .present,
            (plan.blockers ?? []).contains(where: {
                $0.error.code == "env.local_invalid" || $0.error.code == "env.local_unreadable"
            }) == false
        else { return [] }
        return (local.overrides ?? []).filter { $0.state == .applied }
    }

    /// The warnings of the local override file as caution items; an error is a blocker
    /// already. `nil` when there is none.
    static func localWarnings(_ local: PlanLocal) -> [PartialItem]? {
        var seen: Set<String> = []
        let items = (local.findings ?? []).filter { $0.severity == .warning }.map { finding in
            let base = finding.service ?? finding.location
            let name = seen.insert(base).inserted ? base : "\(base) (\(finding.code))"
            return PartialItem(name: name, reason: DaemonText.sentence(finding.message))
        }
        return items.isEmpty ? nil : items
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
    /// The local overrides the plan applies to this item (spec 0045).
    var overrides: [PlanOverride] = []
    let reviewProvider: @MainActor (String) -> Void
    /// Set… of a "Needs secret" line (spec 0046, AC-21), or `nil` for none.
    var setSecret: (@MainActor (PlanSecret) -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                ToneBadge(Self.badge(item.action))
                Text(Self.line(item, displayName: displayName, version: version))
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(
                Self.notes(
                    item, displayName: displayName, overrides: overrides, secrets: false),
                id: \.self
            ) { note in
                Text(note)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .padding(.leading, Spacing.s22)
            }
            ForEach(Self.secretLines(item), id: \.line) { secret in
                HStack(spacing: Spacing.s8) {
                    Text(secret.line)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    if let setSecret, Self.canSet(secret.secret) {
                        Button(EnvironmentCopy.setSecret) { setSecret(secret.secret) }
                            .buttonStyle(.link)
                            .mutationGated()
                    }
                }
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
    /// a create, "· wait for readiness" for a start, and "· {version}" for a keep. A create
    /// whose runtime comes from the lock ends with " · locked" (spec 0045).
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
            let text = parts.joined(separator: " · ")
            return item.runtime?.source == .lock ? text + EnvironmentCopy.lockedSuffix : text
        case .start:
            return "\(head) · wait for readiness"
        case .keep, .unrecognized:
            guard let version = version ?? item.runtime?.version else { return head }
            return "\(head) · \(version)"
        }
    }

    /// The lines under the row: Installs, the local overrides that apply, the differences
    /// (the lock one names the locked version), and, with `secrets`, the secrets.
    static func notes(
        _ item: PlanItem, displayName: (String) -> String, overrides: [PlanOverride] = [],
        secrets: Bool = true
    ) -> [String] {
        var notes: [String] = []
        if item.action == .create, let runtime = item.runtime, runtime.install {
            notes.append("Installs \(displayName(item.provider)) \(runtime.version)")
        }
        if item.action == .create {
            notes.append(contentsOf: overrides.compactMap(EnvironmentCopy.overrideNote))
        }
        for difference in item.differences ?? [] {
            notes.append(
                difference.field == "lock"
                    ? EnvironmentCopy.differsFromLock(difference.manifest)
                    : EnvironmentCopy.differs(difference.field))
        }
        if secrets {
            notes.append(contentsOf: secretLines(item).map(\.line))
        }
        return notes
    }

    /// The secret lines of the row, each with the secret it names.
    static func secretLines(_ item: PlanItem) -> [(line: String, secret: PlanSecret)] {
        (item.secrets ?? []).compactMap { secret in
            EnvironmentCopy.secretLine(secret).map { ($0, secret) }
        }
    }

    /// Whether Set… shows for `secret`: a reference with no value, or one the daemon could
    /// not read.
    static func canSet(_ secret: PlanSecret) -> Bool {
        guard secret.reference != nil else { return false }
        switch secret.status {
        case .missing, .unregistered, .unknown: return true
        case .present, .generated, .unrecognized: return false
        }
    }
}
