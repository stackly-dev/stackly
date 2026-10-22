import Foundation
import StacklyClient

/// What the detail page asks of the daemon and of the shell (spec 0021, *Tab Summary*): the
/// two reads, the stream of `secret.` events, and the secret methods of Show Password and
/// Copy. The app builds one from its ``DaemonModel``; a preview, a snapshot, or a test
/// builds one that answers from data.
nonisolated public struct InstanceDetailService: Sendable {
    /// `instance.get_detail`.
    public var detail: @Sendable (UUID) async throws(ClientError) -> InstanceDetail
    /// `instance.get_data_usage`, with `refresh` and `cachedOnly`.
    public var usage: @Sendable (UUID, Bool, Bool) async throws(ClientError) -> DataUsage?
    /// The `secret.` events from now on, from ``ShellSync/events(kindPrefix:)``.
    public var secretEvents: @MainActor @Sendable () -> AsyncStream<Event>
    /// The secret methods, for `secret.reveal`.
    public var secrets: SecretsClient

    /// Creates a service.
    ///
    /// - Parameters:
    ///   - detail: Reads the detail of one instance.
    ///   - usage: Reads the data size of one instance with `refresh` and `cachedOnly`.
    ///   - secretEvents: Opens the stream of `secret.` events.
    ///   - secrets: The secret methods.
    public init(
        detail: @escaping @Sendable (UUID) async throws(ClientError) -> InstanceDetail,
        usage: @escaping @Sendable (UUID, Bool, Bool) async throws(ClientError) -> DataUsage?,
        secretEvents: @escaping @MainActor @Sendable () -> AsyncStream<Event>,
        secrets: SecretsClient
    ) {
        self.detail = detail
        self.usage = usage
        self.secretEvents = secretEvents
        self.secrets = secrets
    }

    /// A service over the detail methods of a client.
    ///
    /// - Parameters:
    ///   - client: The detail methods.
    ///   - secrets: The secret methods.
    ///   - secretEvents: Opens the stream of `secret.` events.
    public init(
        client: InstanceDetailClient, secrets: SecretsClient,
        secretEvents: @escaping @MainActor @Sendable () -> AsyncStream<Event>
    ) {
        self.init(
            detail: { id throws(ClientError) in try await client.getDetail(instanceId: id) },
            usage: { id, refresh, cachedOnly throws(ClientError) in
                try await client.getDataUsage(
                    instanceId: id, refresh: refresh, cachedOnly: cachedOnly)
            },
            secretEvents: secretEvents, secrets: secrets)
    }
}
