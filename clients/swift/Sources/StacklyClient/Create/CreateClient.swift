import Foundation

/// The two read methods behind the New Instance sheet (spec 0020, *Method IPC*) as typed
/// calls.
///
/// It wraps one `send` closure, the same shape ``SecretsClient`` uses, so a test can answer
/// with a fixed reply and the app builds it over its link. Both methods only read: neither
/// takes an idempotency key, and neither changes the registry, the disk, or the Keychain.
public struct CreateClient: Sendable {
    /// Sends one call and returns the reply.
    public typealias Send = @Sendable (Call, CallOptions) async throws(ClientError) -> Reply

    private let send: Send

    /// Creates a client over `send`.
    ///
    /// - Parameter send: Sends one call with its time limit and returns the reply.
    public init(send: @escaping Send) {
        self.send = send
    }

    /// `instance.get_create_form`: the provider, endpoints, and settings fields a new
    /// instance on `runtimeId` is created from.
    ///
    /// - Parameters:
    ///   - runtimeId: The installed runtime the instance will run on.
    ///   - provider: The provider to use, or `nil` for the catalog entry whose software is
    ///     the software of the runtime.
    /// - Throws: ``ClientError``, such as `instance.runtime_not_found` or
    ///   `catalog.unavailable`.
    public func getCreateForm(
        runtimeId: UUID, provider: String? = nil
    ) async throws(ClientError) -> CreateForm {
        let reply = try await send(
            .instanceGetCreateForm(GetCreateFormParams(runtimeId: runtimeId, provider: provider)),
            .standard)
        guard case .instanceGetCreateForm(let form) = reply else { throw Self.unexpectedReply() }
        return form
    }

    /// `instance.preflight`: the checks, the field errors, and the equivalent command for a
    /// draft, with the same evaluation `instance.create` runs.
    ///
    /// - Parameter draft: The draft. It carries the kind of every secret source and never a
    ///   secret value.
    /// - Throws: ``ClientError``.
    public func preflight(_ draft: PreflightCreateParams) async throws(ClientError)
        -> CreatePreflight
    {
        let reply = try await send(.instancePreflight(draft), .standard)
        guard case .instancePreflight(let preflight) = reply else { throw Self.unexpectedReply() }
        return preflight
    }

    /// A reply of another kind than the call asked for is a broken contract, which the
    /// caller treats as an unknown outcome.
    private static func unexpectedReply() -> ClientError {
        .rejected(ClientErrorCatalog.protocolViolation())
    }
}

extension DaemonLink {
    /// The create read methods over this link, for the app.
    public nonisolated var create: CreateClient {
        CreateClient { [self] call, options throws(ClientError) in
            try await self.call(call, options: options)
        }
    }
}
