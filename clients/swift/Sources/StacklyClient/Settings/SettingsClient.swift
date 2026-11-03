import Foundation

/// The settings and storage calls behind the Settings window (spec 0032, *Method IPC*) as
/// typed methods.
///
/// It wraps one `send` closure, the same shape ``DaemonRestartClient`` uses, so a test can
/// answer with a fixed reply and the app builds it over its link. ``get()`` and
/// ``locations()`` only read. ``update(expectedRevision:patch:idempotencyKey:)`` and
/// ``setLocation(kind:target:expectedRevision:idempotencyKey:)`` are mutations and need the
/// key of the user's action.
public struct SettingsClient: Sendable {
    /// Sends one call with its idempotency key, when it has one, and returns the reply.
    public typealias Send =
        @Sendable (Call, String?, CallOptions) async throws(ClientError) -> Reply

    private let send: Send

    /// Creates a client over `send`.
    ///
    /// - Parameter send: Sends one call with its idempotency key and time limit and returns
    ///   the reply.
    public init(send: @escaping Send) {
        self.send = send
    }

    /// `settings.get`: every daemon setting with its revision.
    ///
    /// - Throws: ``ClientError``.
    public func get() async throws(ClientError) -> DaemonSettingsView {
        let reply = try await send(.settingsGet, nil, .standard)
        guard case .settingsGet(let view) = reply else { throw Self.unexpectedReply() }
        return view
    }

    /// `settings.update`: changes the settings in `patch` when they are still at
    /// `expectedRevision`.
    ///
    /// - Parameters:
    ///   - expectedRevision: The revision the person's screen showed.
    ///   - patch: What to change; at least one field is set.
    ///   - idempotencyKey: The key of the action; a retry after a lost answer reuses it.
    /// - Returns: The settings after the change.
    /// - Throws: ``ClientError``, such as `resource.revision_mismatch`.
    public func update(
        expectedRevision: UInt64, patch: SettingsPatch, idempotencyKey: String
    ) async throws(ClientError) -> DaemonSettingsView {
        let params = UpdateSettingsParams(expectedRevision: expectedRevision, patch: patch)
        let reply = try await send(.settingsUpdate(params), idempotencyKey, .standard)
        guard case .settingsUpdate(let view) = reply else { throw Self.unexpectedReply() }
        return view
    }

    /// `storage.list_locations`: every location the registry knows, the built in ones first.
    ///
    /// - Throws: ``ClientError``.
    public func locations() async throws(ClientError) -> [StorageLocationView] {
        let reply = try await send(.storageListLocations, nil, .standard)
        guard case .storageListLocations(let list) = reply else { throw Self.unexpectedReply() }
        return list.items
    }

    /// `storage.set_location`: points the default of `kind` at `target`.
    ///
    /// - Parameters:
    ///   - kind: Which default to change.
    ///   - target: A folder, or the location that comes with Stackly.
    ///   - expectedRevision: The revision the person's screen showed.
    ///   - idempotencyKey: The key of the action; a retry after a lost answer reuses it.
    /// - Returns: The new default location of `kind`.
    /// - Throws: ``ClientError``, such as `storage.location_invalid` or
    ///   `resource.revision_mismatch`.
    public func setLocation(
        kind: LocationKind, target: LocationTarget, expectedRevision: UInt64,
        idempotencyKey: String
    ) async throws(ClientError) -> StorageLocationView {
        let params = SetLocationParams(
            kind: kind, target: target, expectedRevision: expectedRevision)
        let reply = try await send(.storageSetLocation(params), idempotencyKey, .standard)
        guard case .storageSetLocation(let view) = reply else { throw Self.unexpectedReply() }
        return view
    }

    /// A reply of another kind than the call asked for is a broken contract, which the
    /// caller treats as an unknown outcome.
    private static func unexpectedReply() -> ClientError {
        .rejected(ClientErrorCatalog.protocolViolation())
    }
}

extension DaemonLink {
    /// The settings and storage calls over this link, for the app.
    public nonisolated var settings: SettingsClient {
        SettingsClient { [self] call, key, options throws(ClientError) in
            try await self.call(call, idempotencyKey: key, options: options)
        }
    }
}
