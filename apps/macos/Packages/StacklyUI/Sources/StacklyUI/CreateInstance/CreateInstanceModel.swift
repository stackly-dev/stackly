import CryptoKit
import Foundation
import Observation
import StacklyClient

/// A note above the form of the New Instance sheet: why a create did not go through.
public struct CreateBanner: Equatable {
    /// The tone: critical for a refusal or a failure, caution when the outcome is unknown.
    public let tone: Tone
    /// The title.
    public let title: String
    /// The daemon's words, or `nil`.
    public let message: String?
    /// The error code, or `nil`.
    public let code: String?
}

/// The state of the New Instance sheet (spec 0020, *Sheet New Instance*): the runtime, the
/// form, the draft, the preflight of the current draft, and the create operation.
/// Every change of the draft raises a generation, drops the preflight that runs, and asks
/// again after ``preflightDebounce``; an answer for an older generation is dropped. The model
/// never holds a secret value: the sheet view keeps the typed text and hands it over once,
/// to ``create(secrets:)``.
@Observable
public final class CreateInstanceModel {
    /// Where the sheet is.
    public enum Phase: Equatable {
        /// No runtime is chosen, or none is installed.
        case choosingRuntime
        /// The form and the Copy from list are being read.
        case loadingForm
        /// The form could not be read.
        case formFailed(FailureContent)
        /// The person fills the form.
        case editing
        /// `instance.create` is on its way.
        case submitting
        /// The daemon accepted the operation; the sheet follows it.
        case creating(UUID)
        /// The create succeeded and the sheet is done.
        case finished
    }

    /// How long a draft must stay still before the preflight runs.
    public static let preflightDebounce = Duration.milliseconds(300)

    /// The six stages of `create_instance` as the sheet names them.
    public static let stages: [StageDefinition] = [
        StageDefinition(key: "validate", title: "Checking"),
        StageDefinition(key: "pin", title: "Pinning provider package"),
        StageDefinition(key: "secrets", title: "Storing secrets in Keychain"),
        StageDefinition(key: "layout", title: "Creating folders"),
        StageDefinition(key: "register", title: "Registering"),
        StageDefinition(key: "init", title: "Initializing"),
    ]

    // MARK: - State the views read

    /// Where the sheet is.
    public private(set) var phase: Phase = .choosingRuntime
    /// The category filter.
    public private(set) var category: CreateCategory = .all
    /// The chosen runtime, or `nil`.
    public private(set) var runtimeID: UUID?
    /// The form of the chosen runtime, once it is read.
    public private(set) var form: CreateForm?
    /// The settings form built from ``form``.
    public private(set) var schema: SchemaFormModel?
    /// The name as typed.
    public private(set) var name = ""
    /// The endpoint rows: the IP endpoints of the package.
    public private(set) var endpoints: [EndpointDraft] = []
    /// Whether the instance starts after it is created.
    public private(set) var startAfterCreating = true
    /// The draft generation: raised by every change.
    public private(set) var generation = 0
    /// The newest preflight reply received, of any generation.
    public private(set) var lastPreflight: CreatePreflight?
    /// The generation ``lastPreflight`` answered.
    public private(set) var preflightGeneration: Int?
    /// Why the preflight could not run, or `nil`.
    public private(set) var preflightFailure: ErrorBody?
    /// Whether a preflight for the current generation is waiting or running.
    public private(set) var isPreflightPending = false
    /// The note above the form, or `nil`.
    public private(set) var banner: CreateBanner?
    /// The create operation as the sheet last saw it.
    public private(set) var operation: OperationSummary?
    /// How the sheet started.
    public private(set) var mode: CreateMode = .create(software: nil)
    /// The references Copy from offers, from the daemon's list. A duplicate puts the
    /// reference of its source first for the field that uses one: ``copyFromOptions(for:)``.
    public private(set) var copyFromOptions: [String] = []
    /// The labels of the references that have one; in a duplicate the labels of the source
    /// win over those of the list.
    public private(set) var copyFromLabels: [String: String] = [:]
    /// The kinds of preflight row the failed stage points at, until the draft changes.
    public private(set) var failedStageKinds: Set<PreflightCheckKind> = []

    // MARK: - Collaborators

    @ObservationIgnored private let runtimes: RuntimeStore
    @ObservationIgnored private let catalog: CatalogStore?
    @ObservationIgnored private let instances: InstanceStore
    @ObservationIgnored private let service: any CreateInstanceService
    @ObservationIgnored private let send: @MainActor (Call, String) async -> MutationOutcome
    @ObservationIgnored private let updates: @MainActor (UUID) -> AsyncStream<OperationSnapshot>
    @ObservationIgnored private let debounce: Duration
    @ObservationIgnored private let sleep: (Duration) async throws -> Void
    @ObservationIgnored private let onFinish: @MainActor (CreateOutcome) -> Void

    @ObservationIgnored private var listLabels: [String: String] = [:]
    @ObservationIgnored private var sourceReferences: [String: String] = [:]
    @ObservationIgnored private var sourceLabels: [String: String] = [:]
    @ObservationIgnored private var providerSelector: ProviderSelector?
    @ObservationIgnored private var loadToken = UUID()
    @ObservationIgnored private var formTask: Task<Void, Never>?
    @ObservationIgnored private var listTask: Task<Void, Never>?
    @ObservationIgnored private var preflightTask: Task<Void, Never>?
    @ObservationIgnored private var watchTask: Task<Void, Never>?
    @ObservationIgnored private var retryKey: String?
    @ObservationIgnored private var createKey: String?

    /// Creates the model.
    /// - Parameters:
    ///   - runtimes: The runtimes the picker offers.
    ///   - catalog: The catalog, for the category of a runtime and the tile, or `nil`.
    ///   - instances: The instances, for the labels of Copy from.
    ///   - service: What the sheet reads from the daemon.
    ///   - send: Sends one mutation with its idempotency key.
    ///   - updates: The updates of an operation, ending after its final state.
    ///   - debounce: How long a draft must stay still before the preflight runs.
    ///   - sleep: Waits; tests inject one that does not.
    ///   - onFinish: Called once when a create succeeded and the follow up start was sent.
    public init(
        runtimes: RuntimeStore,
        catalog: CatalogStore?,
        instances: InstanceStore,
        service: any CreateInstanceService,
        send: @escaping @MainActor (Call, String) async -> MutationOutcome,
        updates: @escaping @MainActor (UUID) -> AsyncStream<OperationSnapshot>,
        debounce: Duration = CreateInstanceModel.preflightDebounce,
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) },

        onFinish: @escaping @MainActor (CreateOutcome) -> Void = { _ in }
    ) {
        self.runtimes = runtimes
        self.catalog = catalog
        self.instances = instances
        self.service = service
        self.send = send
        self.updates = updates
        self.debounce = debounce
        self.sleep = sleep
        self.onFinish = onFinish
    }

    // MARK: - Runtimes

    /// The installed runtimes, in no particular order.
    public var installedRuntimes: [RuntimeSummary] {
        runtimes.runtimes.filter { $0.state == .installed }
    }

    /// Whether no runtime is installed at all: the sheet then shows "No Runtimes". A
    /// duplicate has the runtime of its source and never shows it.
    public var hasNoInstalledRuntimes: Bool {
        if case .duplicate = mode { return false }
        return installedRuntimes.isEmpty
    }

    /// The runtimes the picker offers for the category: by display name, then the newest
    /// install first.
    public var visibleRuntimes: [RuntimeSummary] {
        installedRuntimes
            .filter { category.includes(providerCategory(of: $0)) }
            .sorted { a, b in
                switch a.displayName.localizedStandardCompare(b.displayName) {
                case .orderedAscending: return true
                case .orderedDescending: return false
                case .orderedSame:
                    let (left, right) = (
                        a.installedAt ?? .distantPast, b.installedAt ?? .distantPast
                    )
                    return left == right ? a.id.uuidString < b.id.uuidString : left > right
                }
            }
    }

    /// The chosen runtime, or `nil`. A duplicate has the runtime of its form.
    public var runtime: RuntimeSummary? {
        if case .duplicate = mode { return form?.runtime }
        return runtimeID.flatMap { id in installedRuntimes.first { $0.id == id } }
    }

    /// The label of a runtime in the picker: "{display name} {version}", with " (external)"
    /// for a runtime registered from a folder.
    public static func label(of runtime: RuntimeSummary) -> String {
        let base = "\(runtime.displayName) \(runtime.version)"
        return runtime.installMode == .external ? "\(base) (external)" : base
    }

    /// The tile of the chosen runtime, from the catalog entry of its software, or the plain
    /// tile of the runtime when the catalog does not know it.
    public var tile: TileSpec? {
        guard let runtime else { return nil }
        let entry = catalog?.providers?.first { $0.software == runtime.software }
        return TileSpec(
            mark: entry?.mark, tile: entry?.tile,
            name: entry?.displayName ?? runtime.displayName)
    }

    /// The source of a duplicate, or `nil` in any other mode.
    public var duplicateSource: DuplicateSourceRef? {
        if case .duplicate(let source) = mode { source } else { nil }
    }

    /// The read only Runtime row of a duplicate: "{display name} {version} · same as
    /// “{source}”", with " (external)" for a runtime registered from a folder; `nil` before
    /// the form is read and in any other mode.
    public var duplicateRuntimeLabel: String? {
        guard let source = duplicateSource, let form else { return nil }
        let base = "\(form.provider.displayName) \(form.runtime.version)"
        let label = form.runtime.installMode == .external ? "\(base) (external)" : base
        return "\(label) · same as “\(source.name)”"
    }

    /// The line under the title of a duplicate, or `nil` in any other mode.
    public var duplicateSubtitle: String? {
        duplicateSource.map {
            "Settings copied from “\($0.name)”. Its own data directory, ports and secrets; no data is copied."
        }
    }

    private func providerCategory(of runtime: RuntimeSummary) -> ProviderCategory? {
        catalog?.providers?.first { $0.software == runtime.software }?.category
    }

    // MARK: - Opening

    /// Opens the sheet: chooses a runtime and reads its form.
    /// - Parameter software: The software to start on, as the Catalog detail panel asks, or
    ///   `nil`. The runtime is the installed one of that software with the newest install.
    public func begin(software: String? = nil) {
        reset()
        mode = .create(software: software)
        category = .all
        if let software, let runtime = newest(of: software) {
            runtimeID = runtime.id
            if let entry = providerCategory(of: runtime), !category.includes(entry) {
                category = .all
            }
        } else {
            runtimeID = visibleRuntimes.first?.id
        }
        startLoad()
    }

    /// Opens the sheet in a mode: ``begin(software:)`` for a create, ``beginDuplicate(of:)``
    /// for a duplicate.
    /// - Parameter mode: How the sheet starts.
    public func begin(_ mode: CreateMode) {
        switch mode {
        case .create(let software): begin(software: software)
        case .duplicate(let source): beginDuplicate(of: source)
        }
    }

    /// Opens the sheet on the settings of an instance and reads its duplicate form.
    /// - Parameter source: The instance to copy the settings of.
    public func beginDuplicate(of source: DuplicateSourceRef) {
        reset()
        mode = .duplicate(source)
        category = .all
        startLoad()
    }

    /// Stops everything the model runs. Call it when the sheet goes away.
    public func end() {
        reset()
        mode = .create(software: nil)
        phase = .choosingRuntime
    }

    private func newest(of software: String) -> RuntimeSummary? {
        installedRuntimes
            .filter { $0.software == software }
            .max { ($0.installedAt ?? .distantPast) < ($1.installedAt ?? .distantPast) }
    }

    /// Changes the category: keeps the chosen runtime when it belongs, else chooses the
    /// first of the category or none.
    public func selectCategory(_ category: CreateCategory) {
        guard category != self.category else { return }
        self.category = category
        if let runtimeID, visibleRuntimes.contains(where: { $0.id == runtimeID }) { return }
        selectRuntime(visibleRuntimes.first?.id)
    }

    /// Chooses a runtime and reads its form.
    public func selectRuntime(_ id: UUID?) {
        guard id != runtimeID || (id != nil && form == nil) else { return }
        runtimeID = id
        reset(keepRuntime: true)
        startLoad()
    }

    /// Reads the form and the Copy from list again, after a failure.
    public func retryLoad() {
        reset(keepRuntime: true)
        startLoad()
    }

    private func reset(keepRuntime: Bool = false) {
        formTask?.cancel()
        listTask?.cancel()
        preflightTask?.cancel()
        watchTask?.cancel()
        loadToken = UUID()
        if !keepRuntime { runtimeID = nil }
        form = nil
        schema = nil
        name = ""
        endpoints = []
        lastPreflight = nil
        preflightGeneration = nil
        preflightFailure = nil
        isPreflightPending = false
        banner = nil
        operation = nil
        failedStageKinds = []
        copyFromOptions = []
        copyFromLabels = [:]
        listLabels = [:]
        sourceReferences = [:]
        sourceLabels = [:]
        providerSelector = nil
        retryKey = nil
        createKey = nil
        generation += 1
    }

    // MARK: - Loading

    private func startLoad() {
        if case .duplicate(let source) = mode {
            startDuplicateLoad(source)
            return
        }
        guard let runtimeID else {
            phase = .choosingRuntime
            return
        }
        phase = .loadingForm
        let token = loadToken
        let service = service
        formTask = Task { [weak self] in
            do throws(ClientError) {
                let form = try await service.getCreateForm(runtimeId: runtimeID, provider: nil)
                self?.formLoaded(form, token: token)
            } catch {
                self?.formFailed(error, token: token)
            }
        }
        startListLoad(token: token)
    }

    /// Reads the form of the source and the Copy from list together. A list that cannot be
    /// read never fails the form (spec 0024, *Mulai*).
    private func startDuplicateLoad(_ source: DuplicateSourceRef) {
        phase = .loadingForm
        let token = loadToken
        let service = service
        formTask = Task { [weak self] in
            do throws(ClientError) {
                let duplicate = try await service.getDuplicateForm(instanceId: source.id)
                self?.duplicateLoaded(duplicate, token: token)
            } catch {
                self?.formFailed(error, token: token)
            }
        }
        startListLoad(token: token)
    }

    private func startListLoad(token: UUID) {
        let service = service
        listTask = Task { [weak self] in
            // A list that cannot be read hides Copy from and nothing else.
            if let items = try? await service.listSecrets() {
                self?.secretsLoaded(items, token: token)
            }
        }
    }

    private func duplicateLoaded(_ duplicate: DuplicateForm, token: UUID) {
        guard token == loadToken else { return }
        let form = duplicate.form
        self.form = form
        runtimeID = form.runtime.id
        providerSelector = duplicate.draft.provider
        sourceReferences = Dictionary(
            duplicate.secrets.compactMap { secret in
                secret.sourceReference.map { (secret.field, $0) }
            }, uniquingKeysWith: { first, _ in first })
        sourceLabels = sourceReferences.reduce(into: [:]) { labels, entry in
            let title = form.fields.first { $0.key == entry.key }?.title ?? entry.key
            labels[entry.value] = "\(title) of “\(duplicate.source.name)”"
        }
        let schema = SchemaFormModel(fields: form.fields)
        schema.load(settings: duplicate.draft.settings)
        schema.preselectCopyFrom(sourceReferences)
        self.schema = schema
        refreshCopyFrom()
        schema.onChange = { [weak self] in self?.draftDidChange() }
        name = duplicate.draft.name
        let requests = Dictionary(
            (duplicate.draft.endpoints ?? []).map { ($0.name, $0.port) },
            uniquingKeysWith: { first, _ in first })
        endpoints = form.endpoints
            .filter { $0.transport == .tcp || $0.transport == .udp }
            .map { endpoint in
                var row = EndpointDraft(endpoint)
                switch requests[endpoint.name] {
                case .fixed(let port)?:
                    row.mode = .fixed
                    row.portText = String(port)
                case .auto?, nil:
                    row.mode = .automatic
                }
                return row
            }
        phase = .editing
        draftDidChange(immediately: true)
    }
    private func formLoaded(_ form: CreateForm, token: UUID) {
        guard token == loadToken else { return }
        self.form = form
        let schema = SchemaFormModel(fields: form.fields)
        schema.onChange = { [weak self] in self?.draftDidChange() }
        self.schema = schema
        refreshCopyFrom()
        name = form.suggestedName ?? ""
        endpoints = form.endpoints
            .filter { $0.transport == .tcp || $0.transport == .udp }
            .map(EndpointDraft.init)
        phase = .editing
        draftDidChange(immediately: true)
    }

    private func formFailed(_ error: ClientError, token: UUID) {
        guard token == loadToken else { return }
        phase = .formFailed(
            FailureContent(
                title: "Couldn’t Load the Form", message: DaemonText.sentence(error.body.message),
                code: error.body.code))
    }

    private func secretsLoaded(_ items: [SecretRefSummary], token: UUID) {
        guard token == loadToken else { return }
        let offered = items.filter { item in
            (item.status == .present || item.status == .unknown) && !Self.isReserved(item.reference)
        }
        copyFromOptions = offered.map(\.reference)
        var labels: [String: String] = [:]
        for item in offered where item.ownerKind == .instance {
            guard let owner = item.ownerId, let instance = instances.store.items[owner] else {
                continue
            }
            labels[item.reference] = "\(Self.purposeTitle(item.purpose)) of “\(instance.name)”"
        }
        listLabels = labels
        refreshCopyFrom()
    }

    /// The references Copy from offers for a secret field: in a duplicate the reference the
    /// source uses for it first, then the list without that reference; any other field gets
    /// the list as it is (spec 0024, *Secret*).
    /// - Parameter field: The key of the secret field, as `secrets[].field`.
    public func copyFromOptions(for field: String) -> [String] {
        guard let reference = sourceReferences[field] else { return copyFromOptions }
        return [reference] + copyFromOptions.filter { $0 != reference }
    }

    /// Hands the lists and the labels to the form after the list or the form arrived.
    private func refreshCopyFrom() {
        copyFromLabels = listLabels.merging(sourceLabels) { _, source in source }
        guard let schema else { return }
        schema.copyFromOptions = copyFromOptions
        schema.copyFromOptionsByField = Dictionary(
            uniqueKeysWithValues: sourceReferences.keys.map { ($0, copyFromOptions(for: $0)) })
        schema.copyFromLabels = copyFromLabels
    }

    /// Whether a reference belongs to Stackly itself: its first segment is `stackly`
    /// (spec 0011, *Format secret reference*).
    static func isReserved(_ reference: String) -> Bool {
        let path = reference.hasPrefix("local:") ? String(reference.dropFirst(6)) : reference
        return path.split(separator: "/").first == "stackly"
    }

    /// "superuser-password" as "Superuser password".
    static func purposeTitle(_ purpose: String) -> String {
        let words = purpose.replacingOccurrences(of: "-", with: " ")
        return words.prefix(1).uppercased() + words.dropFirst()
    }

    // MARK: - The draft

    /// Sets the name.
    public func setName(_ value: String) {
        guard value != name else { return }
        name = value
        draftDidChange()
    }

    /// Sets Fixed or Automatic for an endpoint.
    public func setMode(_ mode: PortMode, of endpoint: String) {
        update(endpoint) { $0.mode = mode }
    }

    /// Sets the typed port of an endpoint.
    public func setPortText(_ text: String, of endpoint: String) {
        update(endpoint) { $0.portText = text }
    }

    /// Applies the free port the daemon offered, when the person asks for it.
    public func usePort(_ port: UInt16, for endpoint: String) {
        update(endpoint) {
            $0.mode = .fixed
            $0.portText = String(port)
        }
    }

    private func update(_ endpoint: String, _ change: (inout EndpointDraft) -> Void) {
        guard let index = endpoints.firstIndex(where: { $0.name == endpoint }) else { return }
        let before = endpoints[index]
        change(&endpoints[index])
        if endpoints[index] != before { draftDidChange() }
    }

    /// Sets "Start after creating".
    public func setStartAfterCreating(_ value: Bool) {
        guard value != startAfterCreating else { return }
        startAfterCreating = value
        draftDidChange()
    }

    /// The draft the preflight takes, or `nil` before the form is read. A Fixed port that is
    /// not a number is left out; the row says so and Create stays off.
    public func draft() -> PreflightCreateParams? {
        guard let form, let runtimeID, let schema else { return nil }
        return PreflightCreateParams(
            name: name, displayName: nil,
            provider: providerSelector ?? ProviderSelector(name: form.provider.name, revision: nil),
            runtimeId: runtimeID, endpoints: endpoints.compactMap(\.request),
            settings: schema.settings(), secrets: schema.secretPlans(), start: startAfterCreating)
    }

    private func draftDidChange(immediately: Bool = false, fromPerson: Bool = true) {
        generation += 1
        if fromPerson {
            failedStageKinds = []
            retryKey = nil
            createKey = nil
        }
        preflightTask?.cancel()
        guard phase == .editing, let draft = draft() else { return }
        let generation = generation
        preflightFailure = nil
        isPreflightPending = true
        let wait = debounce
        let sleep = sleep
        preflightTask = Task { [weak self] in
            if !immediately {
                try? await sleep(wait)
                if Task.isCancelled { return }
            }
            await self?.runPreflight(draft, generation: generation)
        }
    }

    /// Runs the preflight for `generation` now. The answer counts only while that
    /// generation is still the current one.
    func runPreflight(_ draft: PreflightCreateParams, generation: Int) async {
        do throws(ClientError) {
            let result = try await service.preflight(draft)
            guard generation == self.generation else { return }
            lastPreflight = result
            preflightGeneration = generation
            preflightFailure = nil
            isPreflightPending = false
            schema?.apply(issues: result.fields)
        } catch {
            guard generation == self.generation else { return }
            preflightFailure = error.body
            isPreflightPending = false
        }
    }

    /// Runs the preflight again now, after "Couldn’t run preflight.".
    public func retryPreflight() {
        draftDidChange(immediately: true, fromPerson: false)
    }

    // MARK: - What the sheet shows of the preflight

    /// The preflight of the current draft, or `nil` while it is out of date.
    public var currentPreflight: CreatePreflight? {
        preflightGeneration == generation ? lastPreflight : nil
    }

    /// Whether a draft error of the client keeps Create off: a Fixed port that is not a
    /// number, or a secret outside its limits.
    public var hasClientError: Bool {
        endpoints.contains { $0.hasInvalidPort } || schema?.secretsAreValid == false
    }

    /// Whether the last preflight for the current draft has an error row.
    public var preflightHasError: Bool {
        currentPreflight?.checks.contains { $0.status == .error } == true
    }

    /// The check row for an endpoint of the current preflight.
    public func check(forEndpoint name: String) -> PreflightCheck? {
        (currentPreflight ?? lastPreflight)?.checks.first {
            $0.kind == .endpoint && $0.subject == name
        }
    }

    /// The sentence under the name field, from the issues at `name`.
    public var nameError: String? {
        guard let issue = lastPreflight?.fields.first(where: { $0.path == "name" }) else {
            return nil
        }
        return FieldErrorText.name(issue, name: name)
    }

    // MARK: - Create

    /// Why Create is off, in the order of spec 0020, *Footer*; `nil` when it can go on or
    /// is busy.
    public var createDisabledReason: String? {
        if isCreateBusy { return nil }
        if hasNoInstalledRuntimes { return "Install a runtime first." }
        if case .formFailed = phase { return "Couldn’t load the form." }
        if case .create = mode, runtimeID == nil { return "Choose a runtime." }
        if hasClientError || preflightHasError { return "Fix the errors above." }
        if preflightFailure != nil, currentPreflight == nil {
            return "Run preflight again to continue."
        }
        if currentPreflight == nil { return "Checking…" }
        return nil
    }

    /// Whether the form is loading, or a create is on its way or running.
    public var isCreateBusy: Bool {
        switch phase {
        case .loadingForm, .submitting, .creating, .finished: true
        case .choosingRuntime, .formFailed, .editing: false
        }
    }

    /// Whether the form is read only: while a create runs.
    public var isReadOnly: Bool {
        switch phase {
        case .submitting, .creating, .finished: true
        default: false
        }
    }

    /// The Create button as an action. The sheet view passes the secret inputs it builds
    /// from its drafts; they go to ``create(secrets:)`` and nowhere else.
    /// - Parameters:
    ///   - secrets: Builds the secret inputs from the sheet's drafts at the click.
    ///   - didSend: Called right after the click, to clear the drafts.
    public func createAction(
        secrets: @escaping @MainActor () -> [SecretFieldInput],
        didSend: @escaping @MainActor () -> Void = {}
    ) -> StacklyAction {
        StacklyAction(
            id: "create", title: "Create", mutates: true, disabledReason: createDisabledReason,
            isBusy: isCreateBusy,
            perform: { [self] in
                let inputs = secrets()
                didSend()
                Task { await create(secrets: inputs) }
            })
    }

    /// The Cancel button while a create runs, or `nil` for the sheet's plain Cancel.
    /// While the daemon cannot be reached the sheet cannot send `operation.cancel`, so it
    /// offers Close instead: the sheet is never locked, and the operation keeps running.
    /// - Parameters:
    ///   - gate: The mutation availability of the window.
    ///   - close: Closes the sheet.
    public func cancelAction(
        gate: MutationAvailability, close: @escaping @MainActor () -> Void
    ) -> StacklyAction? {
        guard case .creating(let operationID) = phase else { return nil }
        if gate.blockedReason != nil {
            return StacklyAction(id: "close", title: "Close", mutates: false, perform: close)
        }
        return StacklyAction(
            id: "cancel", title: "Cancel…",
            role: .standard(
                confirmation: ActionConfirmation(
                    title: "Stop creating this instance?",
                    message:
                        "Everything created so far is removed. Initialization in progress finishes first.",
                    actionTitle: "Stop")),
            mutates: true,
            perform: { [self] in Task { await cancel(operationID) } })
    }

    /// Sends `instance.create` with the draft and the secret inputs.
    /// - Parameter secrets: The source of every secret field, with the typed value of an
    ///   Enter field.
    public func create(secrets: [SecretFieldInput]) async {
        guard phase == .editing, createDisabledReason == nil, let form, let runtimeID,
            let schema
        else { return }
        phase = .submitting
        banner = nil
        let key = retryKey ?? MutationPath.newKey()
        createKey = key
        let params = CreateInstanceParams(
            name: name, displayName: nil,
            provider: providerSelector ?? ProviderSelector(name: form.provider.name, revision: nil),
            runtimeId: runtimeID, endpoints: endpoints.compactMap(\.request),
            settings: schema.settings(), secrets: secrets)
        let outcome = await send(.instanceCreate(params), key)
        switch outcome {
        case .completed(let reply):
            guard case .instanceCreate(let accepted) = reply,
                let operation = accepted.accepted?.operation
            else {
                fail(
                    CreateBanner(
                        tone: .critical, title: "Couldn’t create the instance",
                        message: ClientErrorCatalog.protocolViolation().message, code: nil))
                return
            }
            retryKey = nil
            phase = .creating(operation.id)
            self.operation = operation
            follow(operation)
        case .rejected(let body):
            retryKey = nil
            fail(
                CreateBanner(
                    tone: .critical, title: "Couldn’t create the instance",
                    message: DaemonText.sentence(body.message), code: body.code))
        case .notSent:
            retryKey = nil
            fail(
                CreateBanner(
                    tone: .caution, title: MutationFeedback.notSent, message: nil, code: nil))
        case .outcomeUnknown(_, let unknownKey):
            retryKey = unknownKey
            fail(
                CreateBanner(
                    tone: .caution, title: MutationFeedback.outcomeUnknown, message: nil,
                    code: nil))
        }
    }

    /// Returns to the form with `banner` above it and runs the preflight again.
    private func fail(_ banner: CreateBanner) {
        self.banner = banner
        phase = .editing
        draftDidChange(immediately: true, fromPerson: false)
    }

    private func follow(_ operation: OperationSummary) {
        let stream = updates(operation.id)
        watchTask?.cancel()
        watchTask = Task { [weak self] in
            var last = operation
            for await snapshot in stream {
                guard let self, !Task.isCancelled else { return }
                last = snapshot
                self.operation = snapshot
                if Self.isFinal(snapshot.state) { break }
            }
            await self?.operationEnded(last)
        }
    }

    private static func isFinal(_ state: OperationState) -> Bool {
        switch state {
        case .succeeded, .failed, .cancelled, .needsRecovery: true
        case .queued, .running, .cancelRequested: false
        }
    }

    private func operationEnded(_ operation: OperationSummary) async {
        guard case .creating(let id) = phase, id == operation.id else { return }
        switch operation.state {
        case .succeeded:
            await succeeded(operation)
        case .failed:
            failedStageKinds = Self.rowKinds(of: operation.stage)
            let error = operation.failure?.error
            fail(
                CreateBanner(
                    tone: .critical, title: "Couldn’t create the instance",
                    message: error.map { DaemonText.sentence($0.message) },
                    code: error?.code))
        case .cancelled:
            fail(
                CreateBanner(
                    tone: .neutral, title: "Creation was cancelled. Nothing was kept.",
                    message: nil, code: nil))
        case .needsRecovery:
            failedStageKinds = Self.rowKinds(of: operation.stage)
            fail(
                CreateBanner(
                    tone: .caution, title: "The create needs recovery",
                    message: "Open Operations to see what is left and how to clean it up.",
                    code: nil))
        case .queued, .running, .cancelRequested:
            fail(
                CreateBanner(
                    tone: .caution, title: "Lost track of the create",
                    message: "Check Operations to see how it ended.", code: nil))
        }
    }

    private func succeeded(_ operation: OperationSummary) async {
        let created = operation.result?.resources?.first { $0.kind == .instance }
        let instanceID = created.flatMap { UUID(uuidString: $0.id) }
        phase = .finished
        var start = CreateStartResult.notRequested
        if startAfterCreating, let instanceID, let createKey {
            start = await startInstance(instanceID, createKey: createKey)
        }
        onFinish(
            CreateOutcome(
                name: name, instanceID: instanceID, start: start, source: duplicateSource?.name))
    }

    /// The key of the follow up start: the lowercase hex SHA-256 of `<create key>:start`,
    /// the same the CLI derives (spec 0020, *CLI*).
    static func startKey(createKey: String) -> String {
        SHA256.hash(data: Data("\(createKey):start".utf8))
            .map { String(format: "%02x", $0) }.joined()
    }

    private func startInstance(_ id: UUID, createKey: String) async -> CreateStartResult {
        let outcome = await send(
            .instanceStart(StartInstanceParams(instanceId: id, expectedRevision: nil)),
            Self.startKey(createKey: createKey))
        switch outcome {
        case .completed: return .accepted
        case .rejected(let body): return .failed(detail: DaemonText.sentence(body.message))
        case .notSent: return .failed(detail: MutationFeedback.notSent)
        case .outcomeUnknown: return .failed(detail: MutationFeedback.outcomeUnknown)
        }
    }

    private func cancel(_ operationID: UUID) async {
        _ = await send(
            .operationCancel(CancelOperationParams(operationId: operationID)),
            MutationPath.newKey())
    }

    /// The kinds of preflight row a failed stage points at.
    static func rowKinds(of stage: String?) -> Set<PreflightCheckKind> {
        switch stage {
        case "pin": [.provider]
        case "secrets": [.secret]
        case "layout": [.data]
        case "register": [.name]
        case "init": [.settings]
        default: []
        }
    }

    // MARK: - Progress

    /// The progress of the create operation, once it started.
    public var progress: StagedProgressValue? {
        operation.map { StagedProgressValue.from(operation: $0, stages: Self.stages) }
    }

    /// Whether the `secrets` stage is done, so the secret rows read "Stored in Keychain".
    public var secretsAreStored: Bool {
        progress?.stages.first { $0.key == "secrets" }?.state == .done
    }
}
