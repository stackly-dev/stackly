import Foundation
import Observation
import StacklyClient

/// The model of the Configuration tab of one instance (spec 0029, *GUI*): it loads
/// `instance.config.get`, holds the pending change in a ``SchemaFormModel``, judges it with
/// `instance.config.validate` after a pause, and sends Apply, Roll Back, and Keep Revision
/// through the mutation path. It has no provider branch: the form comes from the fields of
/// the pinned package and nothing else.
///
/// A pending change survives a new revision from elsewhere: the tab then says the
/// configuration changed and keeps the change until the person reloads.
@Observable
public final class ConfigurationModel {
    /// The pause between an edit and the validation of it, in milliseconds (spec 0029,
    /// *Konstanta*).
    public static let validateDebounce = Duration.milliseconds(300)

    /// Where the load stands.
    public enum Phase: Equatable, Sendable {
        /// Nothing has been read yet.
        case idle
        /// The first read is on its way.
        case loading
        /// The view is on screen.
        case loaded
        /// The first read failed.
        case failed(ErrorBody)
    }

    /// The two views of the tab.
    public enum Mode: String, CaseIterable, Identifiable, Sendable {
        /// The settings form with Pending Changes and History.
        case form
        /// The files Stackly writes, read only.
        case effectiveFile

        /// The identity of the segment.
        public var id: String { rawValue }

        /// The title of the segment.
        public var title: String {
            switch self {
            case .form: "Form"
            case .effectiveFile: "Effective File"
            }
        }
    }

    /// The mutation that is on its way.
    public enum Busy: Equatable, Sendable {
        /// Apply, including a restore.
        case applying
        /// Roll Back.
        case rollingBack
        /// Keep Revision.
        case keepingRevision
    }

    /// The instance.
    public let instanceId: UUID
    /// Where the load stands.
    public private(set) var phase = Phase.idle
    /// What `instance.config.get` last sent.
    public private(set) var view: InstanceConfigView?
    /// The form, built from the fields of ``view``.
    public private(set) var schema: SchemaFormModel?
    /// The older revision the form was loaded from by Restore, until Revert or Apply.
    public private(set) var restoreSource: ConfigRevisionSummary?
    /// The verdict on the pending change, `nil` while there is none.
    public private(set) var validation: ConfigValidation?
    /// Why the pending change could not be judged, when the daemon refused the question.
    public private(set) var validationError: ErrorBody?
    /// Whether a verdict is on its way.
    public private(set) var isValidating = false
    /// The revision that became active elsewhere while a change was pending.
    public private(set) var conflict: UInt64?
    /// The mutation on its way.
    public private(set) var busy: Busy?
    /// The view of the tab.
    public var mode = Mode.form

    @ObservationIgnored private let service: InstanceConfigService
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private var formBaseline = JSONObject()
    @ObservationIgnored private var validationTask: Task<Void, Never>?
    @ObservationIgnored private var validationRequests = 0
    @ObservationIgnored private var unknown: (call: Call, key: String)?

    /// Creates a model that has read nothing.
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - service: The reads and the mutation path.
    ///   - sleep: Waits for a duration and throws when cancelled; the debounce of the
    ///     validation.
    public init(
        instanceId: UUID, service: InstanceConfigService,
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.instanceId = instanceId
        self.service = service
        self.sleep = sleep
    }

    // MARK: - What the view reads

    /// The number of the active revision, or `nil` before the first read.
    public var activeRevision: UInt64? { view?.revision.number }

    /// Whether the form holds an edit, or a restored revision, that is not applied.
    public var hasPending: Bool {
        guard schema != nil else { return false }
        return restoreSource != nil || formEdited
    }

    /// Whether the form differs from the revision it was loaded from.
    private var formEdited: Bool {
        guard let schema else { return false }
        return schema.settings() != formBaseline
    }

    /// What Apply would do now: the impact of a valid pending change, else none.
    public var impact: ConfigImpact {
        guard hasPending, isValidating == false, let validation, validation.valid else {
            return .none
        }
        return validation.impact
    }

    /// The tag next to the revision number.
    public var impactTag: String { ConfigurationCopy.impactTag(impact) }

    /// Why Apply is off, or `nil` when it can run.
    public var applyDisabledReason: String? {
        if view?.drift != nil {
            return "Configuration files changed outside Stackly. Choose Keep Revision first."
        }
        if conflict != nil { return "This configuration changed. Reload first." }
        if hasPending == false { return "No changes to apply." }
        if isValidating || (validation == nil && validationError == nil) {
            return "Checking your changes…"
        }
        if validationError != nil { return "The changes could not be checked." }
        guard let validation else { return nil }
        if validation.valid == false { return "Fix the errors in the form first." }
        if validation.changes.isEmpty && validation.endpointChanges.isEmpty {
            return "No changes to apply."
        }
        return nil
    }

    /// The last known good revision when the active one never reached ready and a different
    /// one did, for the roll back banner (spec 0029, AC-8).
    public var rollBackTarget: UInt64? {
        guard let view, view.revision.readyAt == nil, let good = view.lastKnownGood,
            good != view.revision.number
        else { return nil }
        return good
    }

    /// The change the form stands for, or `nil` without one: a restore that was not edited
    /// since, else the settings of the active revision with the edits laid over them.
    public var pendingChange: ConfigChangeRequest? {
        guard hasPending, view != nil else { return nil }
        if let restoreSource, formEdited == false { return .restore(restoreSource.number) }
        return .settings(proposedSettings())
    }

    /// The effective files the tab shows: the preview of a valid pending change, else the
    /// files of the active revision.
    public var shownFiles: [EffectiveFile] {
        if hasPending, isValidating == false, let validation, validation.valid,
            validation.preview.isEmpty == false
        {
            return validation.preview
        }
        return view?.effectiveFiles ?? []
    }

    // MARK: - Loading

    /// Reads the configuration once, on the first visit to the tab.
    public func open() async {
        guard phase == .idle else { return }
        await load()
    }

    /// Reads the configuration again and drops every pending change.
    public func reload() async {
        conflict = nil
        await load()
    }

    /// Reads the configuration after the instance changed, without losing a pending change:
    /// a new active revision with a change pending shows the "changed" banner instead.
    public func refresh() async {
        guard phase == .loaded else {
            await open()
            return
        }
        guard busy == nil else { return }
        guard let fresh = try? await service.get(instanceId) else { return }
        guard let current = view else { return }
        if fresh.revision.number == current.revision.number {
            view = fresh
        } else if hasPending {
            conflict = fresh.revision.number
        } else {
            install(fresh)
        }
    }

    private func load() async {
        if view == nil { phase = .loading }
        do throws(ClientError) {
            install(try await service.get(instanceId))
        } catch {
            if view == nil { phase = .failed(error.body) }
        }
    }

    private func install(_ fresh: InstanceConfigView) {
        view = fresh
        restoreSource = nil
        conflict = nil
        validation = nil
        validationError = nil
        isValidating = false
        validationTask?.cancel()
        installSchema(values: fresh.revision.values, of: fresh)
        phase = .loaded
    }

    private func installSchema(values: JSONObject, of fresh: InstanceConfigView) {
        let form = SchemaFormModel(fields: fresh.fields.map(\.field))
        form.load(settings: values)
        var locks: [String: RowLock] = [:]
        var notes: [String: String] = [:]
        for item in fresh.fields {
            let field = item.field
            if field.secret {
                locks[field.key] = .secret(reference: Self.reference(of: values.members[field.key]))
            } else if field.managed {
                locks[field.key] = .managed
            } else if item.locked {
                locks[field.key] = .setOnce
            } else if let note = ConfigurationCopy.fieldNote(
                field, reloadSupported: fresh.reloadSupported)
            {
                notes[field.key] = note
            }
        }
        form.annotate(locks: locks, notes: notes)
        formBaseline = form.settings()
        form.onChange = { [weak self] in self?.formChanged() }
        schema = form
    }

    private static func reference(of value: JSONValue?) -> String {
        if case .string(let text)? = value { return text }
        return "its Keychain entry"
    }

    // MARK: - Editing

    /// Loads the settings of an older revision into the form as a pending change (spec 0029,
    /// AC-12). Apply then runs as usual.
    /// - Parameter revision: A row of History other than the active one.
    public func restore(_ revision: ConfigRevisionSummary) {
        guard let view, revision.isActive == false, busy == nil else { return }
        installSchema(values: revision.values, of: view)
        restoreSource = revision
        schema?.apply(issues: [])
        validation = nil
        validationError = nil
        mode = .form
        scheduleValidation()
    }

    /// Drops the pending change: the form goes back to the active revision.
    public func revert() {
        guard let view, busy == nil else { return }
        validationTask?.cancel()
        restoreSource = nil
        validation = nil
        validationError = nil
        isValidating = false
        installSchema(values: view.revision.values, of: view)
    }

    private func formChanged() {
        scheduleValidation()
    }

    /// The settings of the active revision (or of the restored one) with the edits of the
    /// form laid over them, so a secret, a managed field, and a field the form does not show
    /// travel unchanged.
    private func proposedSettings() -> JSONObject {
        guard let view, let schema else { return JSONObject() }
        var members = (restoreSource?.values ?? view.revision.values).members
        let edited = schema.settings().members
        for key in Set(edited.keys).union(formBaseline.members.keys) {
            let before = formBaseline.members[key]
            let after = edited[key]
            if before != after { members[key] = after }
        }
        return JSONObject(members)
    }

    // MARK: - Validating

    private func scheduleValidation() {
        validationTask?.cancel()
        validationRequests += 1
        guard hasPending else {
            validation = nil
            validationError = nil
            isValidating = false
            schema?.apply(issues: [])
            return
        }
        isValidating = true
        let sleep = self.sleep
        validationTask = Task { [weak self] in
            do { try await sleep(Self.validateDebounce) } catch { return }
            await self?.validateNow()
        }
    }

    /// Judges the pending change now, without the pause. The debounce ends here.
    public func validateNow() async {
        guard let view, let change = pendingChange else { return }
        validationRequests += 1
        let mine = validationRequests
        do throws(ClientError) {
            let verdict = try await service.validate(instanceId, view.revision.number, change)
            guard mine == validationRequests else { return }
            validation = verdict
            validationError = nil
            schema?.apply(issues: verdict.fieldErrors)
            isValidating = false
        } catch {
            guard mine == validationRequests else { return }
            isValidating = false
            validation = nil
            if case .rejected(let body) = error, body.code == "config.revision_conflict" {
                conflict = Self.number(in: body, key: "activeRevision") ?? view.revision.number + 1
            } else {
                validationError = error.body
            }
        }
    }

    private static func number(in body: ErrorBody, key: String) -> UInt64? {
        body.details?.first { $0.key == key }.flatMap { UInt64($0.value) }
    }

    // MARK: - Mutations

    /// Applies the pending change.
    /// - Parameters:
    ///   - name: The name of the instance, for the cards.
    ///   - context: Where the cards go.
    public func apply(name: String, context: InstanceActionContext) async {
        guard busy == nil, applyDisabledReason == nil, let view, let change = pendingChange
        else { return }
        let call = Call.instanceConfigApply(
            ApplyConfigParams(
                instanceId: instanceId, baseRevision: view.revision.number, change: change,
                expectedRevision: nil))
        await mutate(.applying, call, name: name, context: context)
    }

    /// Rolls back to the last known good revision, as a new revision.
    /// - Parameters:
    ///   - name: The name of the instance, for the cards.
    ///   - context: Where the cards go.
    public func rollBack(name: String, context: InstanceActionContext) async {
        guard busy == nil, rollBackTarget != nil, let view else { return }
        let call = Call.instanceConfigApply(
            ApplyConfigParams(
                instanceId: instanceId, baseRevision: view.revision.number, change: .rollback,
                expectedRevision: nil))
        await mutate(.rollingBack, call, name: name, context: context)
    }

    /// Writes the files of the active revision back over the files changed outside Stackly.
    /// - Parameters:
    ///   - name: The name of the instance, for the cards.
    ///   - context: Where the cards go.
    public func keepRevision(name: String, context: InstanceActionContext) async {
        guard busy == nil, view?.drift != nil, let number = activeRevision else { return }
        let call = Call.instanceConfigKeepRevision(KeepRevisionParams(instanceId: instanceId))
        await mutate(.keepingRevision, call, name: name, context: context, keptRevision: number)
    }

    private func mutate(
        _ kind: Busy, _ call: Call, name: String, context: InstanceActionContext,
        keptRevision: UInt64? = nil
    ) async {
        busy = kind
        let key = unknown.flatMap { $0.call == call ? $0.key : nil } ?? MutationPath.newKey()
        let end = await ConfigMutationRunner(service: service).run(call, key: key)
        unknown = nil
        busy = nil
        switch end {
        case .succeeded(let revision, let impact):
            if let keptRevision {
                context.report(
                    .success(
                        "Configuration files restored from revision \(keptRevision).",
                        detail: "\(name) uses them after its next reload or restart."))
            } else {
                context.report(
                    .success(
                        ConfigurationCopy.applied(revision: revision, impact: impact, name: name)))
            }
            restoreSource = nil
            await load()
        case .unchanged:
            context.report(.neutral("Nothing to change on \(name)."))
            await load()
        case .refused(let body):
            absorbRefusal(body, name: name, context: context)
        case .failed(let body, let revision):
            context.report(Self.failure(kind, name: name, revision: revision, body: body))
            restoreSource = nil
            await load()
        case .cancelled:
            context.report(.neutral("The change to \(name) was cancelled."))
            await load()
        case .notCompleted(let outcome):
            if case .outcomeUnknown(_, let key) = outcome { unknown = (call, key) }
            if let card = Feedback.outcome(
                outcome, success: nil, showOperations: context.showOperations)
            {
                context.report(card)
            }
        }
    }

    private func absorbRefusal(_ body: ErrorBody, name: String, context: InstanceActionContext) {
        switch body.code {
        case "config.revision_conflict":
            conflict = Self.number(in: body, key: "activeRevision") ?? (activeRevision ?? 0) + 1
        case "instance.settings_invalid":
            schema?.apply(issues: body.fields ?? [])
            context.report(.failure(body))
        default:
            context.report(.failure(body))
            if body.code == "config.drift_detected" { Task { await refresh() } }
        }
    }

    /// The card of an operation that failed after it wrote its revision.
    static func failure(_ kind: Busy, name: String, revision: UInt64, body: ErrorBody?) -> Feedback
    {
        let what =
            switch kind {
            case .applying: "apply the configuration to"
            case .rollingBack: "roll back"
            case .keepingRevision: "restore the configuration files of"
            }
        var detail = body.map { DaemonText.sentence($0.message) }
        if kind != .keepingRevision, revision > 0 {
            let stays = "Revision \(revision) stays active."
            detail = detail.map { "\($0) \(stays)" } ?? stays
        }
        return Feedback(
            tone: .critical, message: "Couldn’t \(what) \(name).", detail: detail,
            code: body?.code)
    }

    // MARK: - Actions

    /// The Apply button: its title follows the impact, a restart asks first, and a running
    /// apply shows "Applying…".
    /// - Parameters:
    ///   - name: The name of the instance.
    ///   - context: Where the cards go.
    public func applyAction(name: String, context: InstanceActionContext) -> StacklyAction {
        let impact = self.impact
        let isBusy = busy == .applying
        let confirmation: ActionConfirmation? =
            impact == .restart && isBusy == false
            ? ActionConfirmation(
                title: "Apply and restart “\(name)”?",
                message: "Restarting “\(name)” stops it briefly. Its data stays as it is.",
                actionTitle: "Apply and Restart")
            : nil
        return StacklyAction(
            id: "configuration.apply",
            title: isBusy ? ConfigurationCopy.applying : ConfigurationCopy.applyTitle(impact),
            role: .standard(confirmation: confirmation), mutates: true,
            disabledReason: busy == nil ? applyDisabledReason : nil, isBusy: isBusy,
            perform: { [self] in Task { await apply(name: name, context: context) } })
    }

    /// The Revert button: drops the pending change.
    public var revertAction: StacklyAction {
        StacklyAction(
            id: "configuration.revert", title: "Revert", mutates: false,
            disabledReason: hasPending && busy == nil ? nil : "No pending changes.",
            perform: { [self] in revert() })
    }

    /// The Roll Back button of the banner, or `nil` when there is nothing to roll back to.
    /// - Parameters:
    ///   - name: The name of the instance.
    ///   - context: Where the cards go.
    public func rollBackAction(name: String, context: InstanceActionContext) -> StacklyAction? {
        guard let target = rollBackTarget else { return nil }
        let isBusy = busy == .rollingBack
        return StacklyAction(
            id: "configuration.rollBack",
            title: isBusy ? ConfigurationCopy.rollingBack : "Roll Back to Revision \(target)…",
            role: .standard(
                confirmation: isBusy
                    ? nil
                    : ActionConfirmation(
                        title: "Roll back “\(name)” to revision \(target)?",
                        message:
                            "Stackly writes revision \(target) as a new revision and applies it. Its data stays as it is.",
                        actionTitle: "Roll Back")),
            mutates: true,
            disabledReason: busy == nil || isBusy ? nil : "Another change is running.",
            isBusy: isBusy,
            perform: { [self] in Task { await rollBack(name: name, context: context) } })
    }

    /// The Keep Revision button of the drift banner, or `nil` without drift. Cancel is the
    /// default of its confirmation, since it overwrites what was edited outside Stackly.
    /// - Parameters:
    ///   - name: The name of the instance.
    ///   - context: Where the cards go.
    public func keepRevisionAction(name: String, context: InstanceActionContext) -> StacklyAction? {
        guard view?.drift != nil, let number = activeRevision else { return nil }
        let isBusy = busy == .keepingRevision
        return StacklyAction(
            id: "configuration.keepRevision",
            title: isBusy ? ConfigurationCopy.keepingRevision : "Keep Revision \(number)…",
            role: .destructive(
                ActionConfirmation(
                    title: "Keep revision \(number) and overwrite the changed files?",
                    message:
                        "Stackly writes the files of revision \(number) back to disk. What was changed outside Stackly is lost.",
                    actionTitle: "Keep Revision")),
            mutates: true,
            disabledReason: busy == nil || isBusy ? nil : "Another change is running.",
            isBusy: isBusy,
            perform: { [self] in Task { await keepRevision(name: name, context: context) } })
    }
}
