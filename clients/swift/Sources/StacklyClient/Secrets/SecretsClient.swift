/// The secret methods of the daemon (spec 0011, *Permukaan API*) as typed calls.
///
/// It wraps one `call` closure, the same shape the shell's stores use, so a test can answer
/// with a fixed reply and the app passes ``DaemonLink`` through ``DaemonLink/secrets``. No
/// method returns a value except ``revealSecret(reference:purpose:)``, and that one returns
/// a ``RevealedSecret`` that prints as a mask.
public struct SecretsClient: Sendable {
    /// Sends one call and returns the reply.
    public typealias Send = @Sendable (Call, CallOptions) async throws(ClientError) -> Reply

    /// The limit for a call that can wait on a Keychain dialog: the daemon's 180 second
    /// prompt limit (spec 0011 AC-4) plus a margin for the round trip.
    public static let interactiveTimeout: Duration = .seconds(190)

    private let send: Send

    /// Creates a client over `send`.
    ///
    /// - Parameter send: Sends one call with its time limit and returns the reply.
    public init(send: @escaping Send) {
        self.send = send
    }

    /// `secret.list`: the cached reference rows, ordered by reference, and the store.
    ///
    /// - Parameters:
    ///   - ownerKind: Only references of this owner kind, or `nil`.
    ///   - status: Only references with this status, or `nil`.
    /// - Throws: ``ClientError``.
    public func listSecrets(
        ownerKind: SecretOwnerKind? = nil, status: SecretStatus? = nil
    ) async throws(ClientError) -> ListSecretsReply {
        let reply = try await send(
            .secretList(ListSecretsParams(ownerKind: ownerKind, status: status)), .standard)
        guard case .secretList(let list) = reply else { throw Self.unexpectedReply() }
        return list
    }

    /// `secret.status`: one reference after a fresh probe, and the store.
    ///
    /// - Parameter reference: The reference.
    /// - Throws: ``ClientError``, such as `secret.not_found`.
    public func secretStatus(reference: String) async throws(ClientError) -> SecretStatusReply {
        let reply = try await send(
            .secretStatus(SecretStatusParams(reference: reference)), .standard)
        guard case .secretStatus(let status) = reply else { throw Self.unexpectedReply() }
        return status
    }

    /// `secret.set`: fills, generates, copies, or replaces the value behind a reference.
    ///
    /// The reply never carries the value. A short value comes back with the
    /// ``SecretWarning/shortValueNotScrubbed`` warning, and a replace that changes only the
    /// stored copy is refused with `secret.confirmation_required` until `confirmStoredOnly`
    /// is `true`.
    ///
    /// - Parameters:
    ///   - reference: The reference to fill.
    ///   - input: Where the value comes from. `bind` is refused by the daemon here.
    ///   - mode: How to treat a value that exists already.
    ///   - expectedRevision: The revision of the row the caller read, or `nil`.
    ///   - confirmStoredOnly: Confirms an effect that changes only the stored copy.
    /// - Throws: ``ClientError``, such as `secret.conflict` or `secret.invalid_value`.
    public func setSecret(
        reference: String,
        input: SecretInput,
        mode: SecretMode,
        expectedRevision: UInt64? = nil,
        confirmStoredOnly: Bool? = nil
    ) async throws(ClientError) -> SetSecretReply {
        let params = SetSecretParams(
            reference: reference, input: input, mode: mode, expectedRevision: expectedRevision,
            confirmStoredOnly: confirmStoredOnly)
        let reply = try await send(
            .secretSet(params), CallOptions(timeout: Self.interactiveTimeout))
        guard case .secretSet(let set) = reply else { throw Self.unexpectedReply() }
        return set
    }

    /// `secret.reveal`: reads the value behind a reference. The daemon writes one audit row
    /// for every reveal, whether it succeeds or not.
    ///
    /// - Parameters:
    ///   - reference: The reference.
    ///   - purpose: Why the value is read: show it or copy it.
    /// - Returns: The value in a ``RevealedSecret``.
    /// - Throws: ``ClientError``, such as `secret.not_found` or `secret.store_locked`.
    public func revealSecret(
        reference: String, purpose: RevealPurpose
    ) async throws(ClientError) -> RevealedSecret {
        let params = RevealSecretParams(reference: reference, purpose: purpose)
        let reply = try await send(
            .secretReveal(params), CallOptions(timeout: Self.interactiveTimeout))
        guard case .secretReveal(let revealed) = reply, revealed.reference == reference else {
            throw Self.unexpectedReply()
        }
        return RevealedSecret(reference: revealed.reference, value: revealed.value)
    }

    /// `secret.delete`: deletes a reference and the value behind it.
    ///
    /// - Parameters:
    ///   - reference: The reference.
    ///   - expectedRevision: The revision of the row the caller read, or `nil`.
    /// - Throws: ``ClientError``, such as `secret.conflict` while the reference is in use.
    public func deleteSecret(
        reference: String, expectedRevision: UInt64? = nil
    ) async throws(ClientError) {
        let params = DeleteSecretParams(reference: reference, expectedRevision: expectedRevision)
        let reply = try await send(
            .secretDelete(params), CallOptions(timeout: Self.interactiveTimeout))
        guard case .secretDelete = reply else { throw Self.unexpectedReply() }
    }

    /// The error for a reply of the wrong kind: the call may have run, so the outcome is
    /// unknown.
    private static func unexpectedReply() -> ClientError {
        .outcomeUnknown(ClientErrorCatalog.protocolViolation())
    }
}

extension DaemonLink {
    /// The secret methods over this link, for the app.
    public nonisolated var secrets: SecretsClient {
        SecretsClient { [self] call, options throws(ClientError) in
            try await self.call(call, options: options)
        }
    }
}
