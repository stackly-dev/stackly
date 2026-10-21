import Foundation
import Observation
import StacklyClient

/// The state behind ``SecretEntrySheet``: one row per reference the action needs, each
/// saved with `secret.set` in mode `create` (spec 0011, *GUI*).
///
/// The model never holds a value. The sheet owns each row's ``SecretDraft`` and passes it
/// to ``save(reference:draft:)``, ``replace(reference:draft:)``, and
/// ``confirmStoredOnly(reference:draft:)``, which send it and drop it.
@Observable
public final class SecretEntryModel {
    /// Where one row stands.
    public enum RowState: Equatable, Sendable {
        /// Waiting for the person.
        case editing
        /// `secret.set` is running.
        case saving
        /// The value is stored, with the warnings the daemon gave.
        case saved(warnings: [SecretWarning])
        /// A value exists already; the row offers Replace.
        case exists
        /// A replace changes only the stored copy; the sheet asks the person to confirm.
        case needsConfirmation(effects: [SecretEffectInfo])
        /// The save failed, with a safe message.
        case failed(String)
    }

    /// One row of the sheet.
    public struct Row: Identifiable, Equatable, Sendable {
        /// What the row needs.
        public let need: SecretNeed
        /// Where it stands.
        public fileprivate(set) var state = RowState.editing

        /// The reference.
        public var id: String { need.reference }
    }

    /// The rows, in the order of `needs`.
    public private(set) var rows: [Row]

    @ObservationIgnored private let client: SecretsClient
    @ObservationIgnored private var modes: [String: SecretMode] = [:]

    /// Creates a model.
    ///
    /// - Parameters:
    ///   - needs: The references to fill.
    ///   - client: The secret methods.
    public init(needs: [SecretNeed], client: SecretsClient) {
        rows = needs.map { Row(need: $0) }
        self.client = client
    }

    /// Whether every row is saved, so the main button may repeat the first action.
    public var allSaved: Bool {
        !rows.isEmpty
            && rows.allSatisfy {
                if case .saved = $0.state { true } else { false }
            }
    }

    /// The row waiting for a stored-only confirmation, if any.
    public var pendingConfirmation: (reference: String, effects: [SecretEffectInfo])? {
        for row in rows {
            if case .needsConfirmation(let effects) = row.state { return (row.id, effects) }
        }
        return nil
    }

    /// Saves the row with mode `create`.
    public func save(reference: String, draft: SecretDraft) async {
        await send(reference: reference, draft: draft, mode: .create, confirm: nil)
    }

    /// Saves the row with mode `replace`, after `secret.conflict` said a value exists.
    public func replace(reference: String, draft: SecretDraft) async {
        await send(reference: reference, draft: draft, mode: .replace, confirm: nil)
    }

    /// Sends the last save again with `confirmStoredOnly`, after the person confirmed that
    /// the password inside the database does not change.
    public func confirmStoredOnly(reference: String, draft: SecretDraft) async {
        await send(
            reference: reference, draft: draft, mode: modes[reference] ?? .replace, confirm: true)
    }

    /// Takes the row back to editing, after the person declined a confirmation.
    public func cancelConfirmation(reference: String) {
        update(reference) {
            if case .needsConfirmation = $0 { $0 = .exists }
        }
    }

    private func send(
        reference: String, draft: SecretDraft, mode: SecretMode, confirm: Bool?
    ) async {
        guard let row = rows.first(where: { $0.id == reference }),
            let input = draft.input(limits: row.need.limits),
            row.state != .saving
        else { return }
        modes[reference] = mode
        update(reference) { $0 = .saving }
        do throws(ClientError) {
            let reply = try await client.setSecret(
                reference: reference, input: input, mode: mode, confirmStoredOnly: confirm)
            update(reference) { $0 = .saved(warnings: reply.warnings ?? []) }
        } catch {
            update(reference) { $0 = Self.state(after: error) }
        }
    }

    private func update(_ reference: String, _ change: (inout RowState) -> Void) {
        guard let index = rows.firstIndex(where: { $0.id == reference }) else { return }
        change(&rows[index].state)
    }

    /// The row state after a failed `secret.set`.
    static func state(after error: ClientError) -> RowState {
        switch error {
        case .notSent:
            return .failed("Couldn’t reach stacklyd. Nothing was saved.")
        case .outcomeUnknown:
            return .failed("stacklyd didn’t answer in time. Check the status, then try again.")
        case .rejected(let body):
            let facts = Dictionary(
                (body.details ?? []).map { ($0.key, $0.value) },
                uniquingKeysWith: { first, _ in first })
            switch body.code {
            case "secret.conflict" where facts["reason"] == "already_exists":
                return .exists
            case "secret.confirmation_required":
                return .needsConfirmation(effects: effects(in: body))
            default:
                return .failed(
                    DaemonText.sentence(body.message)
                        + (body.hint.map { " " + DaemonText.sentence($0) } ?? ""))
            }
        }
    }

    /// The effects a `secret.confirmation_required` error lists.
    private static func effects(in body: ErrorBody) -> [SecretEffectInfo] {
        (body.items ?? []).compactMap { item in
            guard item.kind == "effect" else { return nil }
            let facts = Dictionary(
                (item.fields ?? []).map { ($0.key, $0.value) },
                uniquingKeysWith: { first, _ in first })
            guard let id = facts["resourceId"], let field = facts["field"],
                let kind = facts["resourceKind"].flatMap(ResourceKind.init(rawValue:)),
                let effect = facts["effect"].flatMap(SecretEffectKind.init(rawValue:))
            else { return nil }
            return SecretEffectInfo(
                resourceKind: kind, resourceId: id, field: field, effect: effect)
        }
    }
}
