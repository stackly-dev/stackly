import Foundation
import StacklyClient

/// Copies a connection string for a menu item or a command (spec 0021, AC-13): the Copy
/// Connection String item of the More menu, and the command of the same name.
///
/// A string with a secret is revealed with `secret.reveal { purpose: copy }`, joined by
/// ``ConnectionComposer``, and written by ``ConcealedPasteboard``; one without goes to the
/// plain pasteboard. Nothing is revealed or written until this runs, and a reveal that
/// fails copies nothing.
@MainActor
public enum ConnectionCopier {
    /// Copies `connection` and says how it went.
    ///
    /// - Parameters:
    ///   - connection: The entry to copy.
    ///   - secrets: The secret methods.
    ///   - pasteboard: Where a string with a secret goes.
    ///   - write: Writes a string without a secret to the plain pasteboard.
    /// - Returns: The feedback card for the person: good news, or why nothing was copied.
    public static func copy(
        _ connection: ConnectionString,
        secrets: SecretsClient,
        pasteboard: ConcealedPasteboard = .shared,
        write: @MainActor (String) -> Void = PlainPasteboard.write
    ) async -> Feedback {
        if let reason = DetailFormat.unavailableText(for: connection) {
            return failure(connection, detail: reason)
        }
        let references = ConnectionComposer.references(in: connection.parts)
        if references.isEmpty {
            do throws(ConnectionComposer.Failure) {
                write(try ConnectionComposer.compose(parts: connection.parts, values: [:]))
            } catch {
                return failure(connection)
            }
            return .success("\(connection.title) copied.")
        }
        let model = RevealModel(
            references: references,
            reveal: { reference, purpose throws(ClientError) in
                try await secrets.revealSecret(reference: reference, purpose: purpose)
            },
            compose: ConnectionComposer.composer(for: connection.parts),
            pasteboard: pasteboard)
        await model.copy()
        switch model.phase {
        case .copied:
            return .success(
                "\(connection.title) copied.", detail: "The clipboard clears in 90 seconds.")
        case .failed(let message):
            return failure(connection, detail: message)
        case .idle, .working:
            return failure(connection)
        }
    }

    private static func failure(_ connection: ConnectionString, detail: String? = nil) -> Feedback {
        Feedback(
            tone: .critical, message: "Couldn’t copy \(connection.title).", detail: detail)
    }
}

extension ConnectionCopier {
    /// The Copy Connection String action of the More menu and of the command of the same
    /// name: it copies the first connection string, and is off with the reason when there is
    /// none or it cannot be copied. It changes no data, so the mutation gate leaves it on.
    ///
    /// - Parameters:
    ///   - detail: The detail of the instance, or `nil` while it has not loaded.
    ///   - secrets: The secret methods.
    ///   - report: Shows the result to the person.
    public static func action(
        detail: InstanceDetail?,
        secrets: SecretsClient,
        report: @escaping @MainActor (Feedback) -> Void
    ) -> StacklyAction {
        let primary = detail.flatMap(DetailFormat.primaryConnection)
        return StacklyAction(
            id: CommandID.copyConnectionString.rawValue, title: "Copy Connection String",
            mutates: false, disabledReason: DetailFormat.copyConnectionReason(detail),
            perform: {
                guard let primary else { return }
                Task { report(await copy(primary, secrets: secrets)) }
            })
    }
}
