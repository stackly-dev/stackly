import Foundation
import Observation
import StacklyClient

/// Cancel, Resume, and Abandon of the Operations page (spec 0035, *Halaman Operations*): each
/// click sends one mutation with a new idempotency key, puts the summary the daemon answers
/// with into the history, and reports how it went in a feedback card (spec 0067). A click
/// marks the operation busy until the reply came, so a second click sends nothing.
@Observable
public final class OperationActions {
    /// Which mutation runs for an operation.
    public enum Kind: Equatable, Sendable {
        /// `operation.cancel`.
        case cancel
        /// `operation.resume`.
        case resume
        /// `operation.abandon`.
        case abandon
    }

    /// The mutation that runs for each operation, until its reply came.
    public private(set) var inFlight: [UUID: Kind] = [:]

    @ObservationIgnored private let send: @MainActor (Call, String) async -> MutationOutcome
    @ObservationIgnored private let apply: @MainActor (OperationSnapshot) -> Void
    @ObservationIgnored private let reread: @MainActor (UUID) async -> Void

    /// Creates the actions.
    /// - Parameters:
    ///   - send: Sends one mutation with its idempotency key.
    ///   - apply: Puts a newer summary into the history.
    ///   - reread: Reads one operation again, after the daemon refused a stale revision.
    public init(
        send: @escaping @MainActor (Call, String) async -> MutationOutcome,
        apply: @escaping @MainActor (OperationSnapshot) -> Void,
        reread: @escaping @MainActor (UUID) async -> Void
    ) {
        self.send = send
        self.apply = apply
        self.reread = reread
    }

    /// The actions of a page: the mutations of `service`, and the replies into `history`.
    /// - Parameters:
    ///   - service: The page's service.
    ///   - history: The page's history.
    public convenience init(service: OperationsService, history: OperationHistory) {
        self.init(
            send: service.send,
            apply: { history.apply($0) },
            reread: { _ = await history.read($0) })
    }

    // MARK: - Clicks

    /// Sends `operation.cancel` (no confirmation: a cancel undoes or stops safely).
    /// - Parameters:
    ///   - operation: The operation as the page shows it.
    ///   - title: Its title, for the card.
    ///   - report: Shows a feedback card.
    public func cancel(
        _ operation: OperationSnapshot, title: String,
        report: @escaping @MainActor (Feedback) -> Void
    ) async {
        let call = Call.operationCancel(CancelOperationParams(operationId: operation.id))
        await run(.cancel, operation, call: call, report: report) { reply in
            guard case .operationCancel(let cancelled) = reply else { return nil }
            return (cancelled.operation, Self.feedback(cancelled.disposition, title: title))
        }
    }

    /// Sends `operation.resume` with the revision the page shows.
    /// - Parameters:
    ///   - operation: The operation as the page shows it.
    ///   - title: Its title, for the card.
    ///   - report: Shows a feedback card.
    public func resume(
        _ operation: OperationSnapshot, title: String,
        report: @escaping @MainActor (Feedback) -> Void
    ) async {
        let call = Call.operationResume(
            ResumeOperationParams(operationId: operation.id, expectedRevision: operation.revision))
        await run(.resume, operation, call: call, report: report) { reply in
            guard case .operationResume(let resumed) = reply else { return nil }
            return (resumed, Feedback.started("Resuming “\(title)”."))
        }
    }

    /// Sends `operation.abandon` with the revision the page shows. The page asks first with
    /// the destructive confirmation of ``abandonConfirmation(title:)``.
    /// - Parameters:
    ///   - operation: The operation as the page shows it.
    ///   - title: Its title, for the card.
    ///   - report: Shows a feedback card.
    public func abandon(
        _ operation: OperationSnapshot, title: String,
        report: @escaping @MainActor (Feedback) -> Void
    ) async {
        let call = Call.operationAbandon(
            AbandonOperationParams(operationId: operation.id, expectedRevision: operation.revision)
        )
        await run(.abandon, operation, call: call, report: report) { reply in
            guard case .operationAbandon(let abandoned) = reply else { return nil }
            return (
                abandoned.operation,
                Self.abandoned(title: title, leftovers: abandoned.leftovers?.count ?? 0)
            )
        }
    }

    private func run(
        _ kind: Kind, _ operation: OperationSnapshot, call: Call,
        report: @escaping @MainActor (Feedback) -> Void,
        read: (Reply) -> (OperationSnapshot, Feedback?)?
    ) async {
        guard inFlight[operation.id] == nil else { return }
        inFlight[operation.id] = kind
        defer { inFlight[operation.id] = nil }
        let outcome = await send(call, MutationPath.newKey())
        switch outcome {
        case .completed(let reply):
            guard let (summary, card) = read(reply) else {
                report(Feedback.failure(ClientErrorCatalog.protocolViolation()))
                return
            }
            apply(summary)
            if let card { report(card) }
        case .rejected(let body):
            if body.code == "resource.revision_mismatch" {
                // The page showed an older revision: read it again before the card shows.
                await reread(operation.id)
            }
            report(Feedback.failure(body))
        case .notSent, .outcomeUnknown:
            if let card = Feedback.outcome(outcome, success: nil, showOperations: nil) {
                report(card)
            }
        }
    }

    // MARK: - Copy

    /// The card after a cancel, by what the daemon did with it.
    /// - Parameters:
    ///   - disposition: The disposition of the reply.
    ///   - title: The title of the operation.
    static func feedback(_ disposition: CancelDisposition, title: String) -> Feedback? {
        switch disposition {
        case .cancelledBeforeStart:
            .neutral("Cancelled “\(title)”. It hadn’t started, so nothing of it ran.")
        case .requested:
            .neutral("Cancelling “\(title)”. It stops at its next safe point.")
        case .continuingPastPointOfNoReturn:
            Feedback(
                tone: .caution,
                message: "It’s past a step that can’t be undone, so it will finish on its own.")
        case .alreadyFinal:
            .neutral("“\(title)” had already ended.")
        case .recoveryRequired:
            Feedback(tone: .caution, message: "“\(title)” needs recovery. Resume or abandon it.")
        case .unrecognized:
            nil
        }
    }

    /// The card after an abandon, with the number of items left as they are.
    static func abandoned(title: String, leftovers: Int) -> Feedback {
        switch leftovers {
        case 0: .success("Abandoned “\(title)”.")
        case 1: .success("Abandoned “\(title)”.", detail: "1 item was left as it is.")
        default:
            .success("Abandoned “\(title)”.", detail: "\(leftovers) items were left as they are.")
        }
    }

    /// The confirmation of Abandon (spec 0035, *Konfirmasi*): destructive, Cancel as the
    /// default.
    /// - Parameter title: The title of the operation.
    public static func abandonConfirmation(title: String) -> ActionConfirmation {
        ActionConfirmation(
            title: "Abandon “\(title)”?",
            message:
                "Stackly undoes what it can and leaves the rest as it is. You can’t resume it afterwards.",
            actionTitle: "Abandon")
    }
}
