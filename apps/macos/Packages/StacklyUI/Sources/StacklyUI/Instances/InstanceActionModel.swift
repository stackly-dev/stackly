import Foundation
import Observation
import StacklyClient

/// Where an instance action reports and how it opens Operations; the page builds it from
/// its environment (the window's ``FeedbackCenter`` and the ``ShellNavigator``).
public struct InstanceActionContext {
    /// Shows a feedback card.
    public var report: @MainActor (Feedback) -> Void
    /// Opens Operations, for the "Show Operations" button of an unknown outcome.
    public var showOperations: (@MainActor () -> Void)?

    /// Creates a context.
    public init(
        report: @escaping @MainActor (Feedback) -> Void,
        showOperations: (@MainActor () -> Void)? = nil
    ) {
        self.report = report
        self.showOperations = showOperations
    }
}

/// Start, Stop, and Retry for instances (spec 0022, *Aksi instance*): one per
/// ``DaemonModel``, so a click survives a change of page.
///
/// A click marks the instance in flight at once, so a second click while the reply is on its
/// way sends nothing. An accepted click stays pending until its operation reaches a final
/// state. A key whose outcome is unknown is reused for the same action on the same instance
/// while its `revision` has not moved and for at most five minutes.
@Observable
public final class InstanceActionModel {
    /// How long a key whose outcome is unknown can be reused, in seconds.
    static var keyLifetime: TimeInterval { 300 }

    /// The instance and the verb a stored key belongs to.
    private struct KeyID: Hashable {
        let instance: UUID
        let verb: InstanceVerb
    }

    /// A key kept after an unknown outcome.
    private struct StoredKey {
        let key: String
        let revision: UInt64
        let at: Date
    }

    private var statuses: [UUID: InstanceLocalStatus] = [:]
    @ObservationIgnored private var keys: [KeyID: StoredKey] = [:]
    @ObservationIgnored private var watchers: [UUID: Task<Void, Never>] = [:]

    @ObservationIgnored private let send: @MainActor (Call, String) async -> MutationOutcome
    @ObservationIgnored private let updates: @MainActor (UUID) -> AsyncStream<OperationSnapshot>
    @ObservationIgnored private let readOperation: @MainActor (UUID) async -> OperationSnapshot?
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private let announce: @MainActor (String) -> Void

    /// Creates the model.
    ///
    /// - Parameters:
    ///   - send: Sends one mutation with its idempotency key.
    ///   - updates: The updates of an operation, ending after its final state.
    ///   - readOperation: One `operation.get`, or `nil` when it fails.
    ///   - now: The wall clock.
    ///   - announce: Posts a VoiceOver announcement.
    public init(
        send: @escaping @MainActor (Call, String) async -> MutationOutcome,
        updates: @escaping @MainActor (UUID) -> AsyncStream<OperationSnapshot>,
        readOperation: @escaping @MainActor (UUID) async -> OperationSnapshot?,
        now: @escaping () -> Date = { Date() },
        announce: @escaping @MainActor (String) -> Void = { StacklyUI.announce($0) }
    ) {
        self.send = send
        self.updates = updates
        self.readOperation = readOperation
        self.now = now
        self.announce = announce
    }

    // MARK: - What the views read

    /// What this window knows about its own click for an instance.
    public func localStatus(for id: UUID) -> InstanceLocalStatus {
        statuses[id] ?? .none
    }

    /// The main action of an instance now.
    public func primaryAction(for summary: InstanceSummary) -> InstanceAction {
        InstanceAction.resolve(summary: summary, local: localStatus(for: summary.id))
    }

    /// The main action of an instance as a ``StacklyAction``: the button of a row, the
    /// button of the inspector, and the Start and Stop commands all read it.
    ///
    /// - Parameters:
    ///   - summary: The instance.
    ///   - context: Where it reports.
    public func stacklyAction(for summary: InstanceSummary, context: InstanceActionContext)
        -> StacklyAction
    {
        let action = primaryAction(for: summary)
        let id =
            action.kind == .busy ? (action.commandVerb?.rawValue ?? "busy") : action.kind.rawValue
        return StacklyAction(
            id: id, title: action.title, symbol: action.symbol, mutates: true,
            disabledReason: action.disabledReason, isBusy: action.isBusy,
            perform: { [self] in
                if let verb = action.verb {
                    perform(verb, for: summary, context: context)
                }
            })
    }

    // MARK: - Clicks

    /// Sends a click, unless one is in flight or pending for the instance.
    ///
    /// - Returns: The task that runs until the click ended, or `nil` when nothing was sent.
    @discardableResult
    public func perform(
        _ verb: InstanceVerb, for summary: InstanceSummary, context: InstanceActionContext
    ) -> Task<Void, Never>? {
        guard localStatus(for: summary.id) == .none else { return nil }
        statuses[summary.id] = .inFlight(verb)
        let key = key(for: verb, summary: summary)
        let task = Task { [self] in
            await run(verb, summary: summary, key: key, context: context)
        }
        watchers[summary.id] = task
        return task
    }

    private func key(for verb: InstanceVerb, summary: InstanceSummary) -> String {
        let id = KeyID(instance: summary.id, verb: verb)
        if let stored = keys[id] {
            if stored.revision == summary.revision,
                now().timeIntervalSince(stored.at) <= Self.keyLifetime
            {
                return stored.key
            }
            keys[id] = nil
        }
        return MutationPath.newKey()
    }

    private func call(_ verb: InstanceVerb, for id: UUID) -> Call {
        switch verb {
        case .start: .instanceStart(StartInstanceParams(instanceId: id, expectedRevision: nil))
        case .stop:
            .instanceStop(
                StopInstanceParams(instanceId: id, expectedRevision: nil, overrideLeases: false))
        }
    }

    private func run(
        _ verb: InstanceVerb, summary: InstanceSummary, key: String, context: InstanceActionContext
    ) async {
        let id = summary.id
        let outcome = await send(call(verb, for: id), key)
        guard Task.isCancelled == false else { return }
        let keyID = KeyID(instance: id, verb: verb)
        switch outcome {
        case .completed(let reply):
            keys[keyID] = nil
            guard let accepted = Self.accepted(in: reply), accepted.alreadyInState == false,
                let operation = accepted.accepted?.operation
            else {
                clear(id)
                return
            }
            announce("\(verb.progressive) \(summary.name).")
            statuses[id] = .pending(verb, operationId: operation.id)
            await watch(
                operation, instance: id, verb: verb, name: summary.name, context: context)
        case .rejected(let body):
            keys[keyID] = nil
            clear(id)
            context.report(Self.failure(verb: verb, name: summary.name, body: body))
        case .notSent:
            clear(id)
            report(outcome, context: context)
        case .outcomeUnknown(_, let unknownKey):
            keys[keyID] = StoredKey(key: unknownKey, revision: summary.revision, at: now())
            clear(id)
            report(outcome, context: context)
        }
    }

    private static func accepted(in reply: Reply) -> InstanceActionAccepted? {
        switch reply {
        case .instanceStart(let accepted), .instanceStop(let accepted): accepted
        default: nil
        }
    }

    private func report(_ outcome: MutationOutcome, context: InstanceActionContext) {
        if let feedback = Feedback.outcome(
            outcome, success: nil, showOperations: context.showOperations)
        {
            context.report(feedback)
        }
    }

    /// Follows the operation until it reaches a final state, or needs recovery (which Needs
    /// Attention shows), and reports how it ended.
    private func watch(
        _ operation: OperationSnapshot, instance: UUID, verb: InstanceVerb, name: String,
        context: InstanceActionContext
    ) async {
        var last = operation
        if Self.isWaiting(operation) {
            for await update in updates(operation.id) {
                last = update
                if Self.isWaiting(update) == false { break }
            }
        }
        guard Task.isCancelled == false else { return }
        if Self.isWaiting(last), let read = await readOperation(operation.id) {
            // The stream ended without a final state: the daemon tells how it ended.
            last = read
        }
        finish(last, verb: verb, name: name, context: context)
        clear(instance)
    }

    /// Whether an operation has not ended and does not wait for recovery.
    private static func isWaiting(_ operation: OperationSnapshot) -> Bool {
        OperationCenter.isActive(operation.state) && operation.state != .needsRecovery
    }

    private func finish(
        _ operation: OperationSnapshot, verb: InstanceVerb, name: String,
        context: InstanceActionContext
    ) {
        switch operation.state {
        case .failed:
            let message = "Couldn’t \(verb.word) “\(name)”."
            if let body = operation.failure?.error {
                context.report(Self.failure(verb: verb, name: name, body: body))
            } else {
                context.report(Feedback(tone: .critical, message: message))
            }
        case .cancelled:
            context.report(
                Feedback.neutral("“\(name)” didn’t \(verb.word): the operation was cancelled."))
        case .succeeded, .queued, .running, .cancelRequested, .needsRecovery:
            break
        }
    }

    /// The card of a click the daemon refused or an operation that failed: it names the
    /// instance, then the daemon's message as a sentence and its code.
    static func failure(verb: InstanceVerb, name: String, body: ErrorBody) -> Feedback {
        Feedback(
            tone: .critical,
            message: "Couldn’t \(verb.word) “\(name)”.",
            detail: DaemonText.sentence(body.message),
            code: body.code)
    }

    private func clear(_ id: UUID) {
        statuses[id] = nil
        watchers[id] = nil
    }

    // MARK: - After a resnapshot

    /// Ends every pending click whose operation is no longer active: it may have ended while
    /// the shell was away. One `operation.get` per click tells how it ended.
    ///
    /// Call it when ``ShellSync/completedRounds`` changes.
    ///
    /// - Parameters:
    ///   - activeOperations: The IDs ``OperationCenter/active`` holds now.
    ///   - names: The instance names by ID, for the feedback.
    ///   - context: Where it reports.
    public func reconcile(
        activeOperations: Set<UUID>, names: [UUID: String], context: InstanceActionContext
    ) async {
        for (id, status) in statuses {
            guard case .pending(let verb, let operationId) = status,
                activeOperations.contains(operationId) == false
            else { continue }
            watchers[id]?.cancel()
            let ended = await readOperation(operationId)
            clear(id)
            if let ended {
                finish(ended, verb: verb, name: names[id] ?? "instance", context: context)
            }
        }
    }
}
