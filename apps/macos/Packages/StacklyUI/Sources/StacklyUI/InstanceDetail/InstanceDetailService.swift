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
    /// `instance.get`: the summary of an instance the store does not hold, or `nil` when the
    /// daemon does not know it or it is retained.
    public var instance: @Sendable (UUID) async throws(ClientError) -> InstanceSummary?
    /// The secret methods, for `secret.reveal`.
    public var secrets: SecretsClient

    /// Creates a service.
    ///
    /// - Parameters:
    ///   - detail: Reads the detail of one instance.
    ///   - usage: Reads the data size of one instance with `refresh` and `cachedOnly`.
    ///   - instance: Reads one instance summary, or `nil` when it is gone.
    ///   - secretEvents: Opens the stream of `secret.` events.
    ///   - secrets: The secret methods.
    public init(
        detail: @escaping @Sendable (UUID) async throws(ClientError) -> InstanceDetail,
        usage: @escaping @Sendable (UUID, Bool, Bool) async throws(ClientError) -> DataUsage?,
        instance: @escaping @Sendable (UUID) async throws(ClientError) -> InstanceSummary?,
        secretEvents: @escaping @MainActor @Sendable () -> AsyncStream<Event>,
        secrets: SecretsClient
    ) {
        self.detail = detail
        self.usage = usage
        self.instance = instance
        self.secretEvents = secretEvents
        self.secrets = secrets
    }

    /// A service over the detail methods of a client.
    ///
    /// - Parameters:
    ///   - client: The detail methods.
    ///   - secrets: The secret methods.
    ///   - instance: Reads one instance summary, or `nil` when it is gone.
    ///   - secretEvents: Opens the stream of `secret.` events.
    public init(
        client: InstanceDetailClient, secrets: SecretsClient,
        instance: @escaping @Sendable (UUID) async throws(ClientError) -> InstanceSummary?,
        secretEvents: @escaping @MainActor @Sendable () -> AsyncStream<Event>
    ) {
        self.init(
            detail: { id throws(ClientError) in try await client.getDetail(instanceId: id) },
            usage: { id, refresh, cachedOnly throws(ClientError) in
                try await client.getDataUsage(
                    instanceId: id, refresh: refresh, cachedOnly: cachedOnly)
            },
            instance: instance, secretEvents: secretEvents, secrets: secrets)
    }
}

extension InstanceDetailService {
    /// The service of the running app: the reads go through the model's link, the events come
    /// from its ``ShellSync``, and the secret methods are its own.
    ///
    /// - Parameter model: The daemon model of the window.
    static func live(_ model: DaemonModel) -> InstanceDetailService {
        InstanceDetailService(
            detail: { id throws(ClientError) in
                let params = GetInstanceDetailParams(instanceId: id)
                let reply = try await model.read(.instanceGetDetail(params)).get()
                guard case .instanceGetDetail(let detail) = reply else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return detail
            },
            usage: { id, refresh, cachedOnly throws(ClientError) in
                let params = GetDataUsageParams(
                    instanceId: id, refresh: refresh, cachedOnly: cachedOnly)
                let reply = try await model.read(.instanceGetDataUsage(params)).get()
                guard case .instanceGetDataUsage(let usage) = reply else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return usage.usage
            },
            instance: { id throws(ClientError) in
                do throws(ClientError) {
                    let reply = try await model.read(
                        .instanceGet(GetInstanceParams(instanceId: id))
                    )
                    .get()
                    guard case .instanceGet(let summary) = reply else {
                        throw .rejected(ClientErrorCatalog.protocolViolation())
                    }
                    return summary.lifecycle == .retained ? nil : summary
                } catch {
                    if case .rejected(let body) = error, body.code == "resource.not_found" {
                        return nil
                    }
                    throw error
                }
            },
            secretEvents: { model.sync.events(kindPrefix: "secret.") },
            secrets: model.secrets)
    }
}
