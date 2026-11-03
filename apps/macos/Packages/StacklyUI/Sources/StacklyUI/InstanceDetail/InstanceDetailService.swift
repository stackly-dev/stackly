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
    /// `instance.prepare_terminal`: the path of the `.command` script of a `terminal` Open In
    /// action (spec 0070, AC-4).
    public var prepareTerminal: @Sendable (UUID, String) async throws(ClientError) -> String
    /// `instance.set_startup`: sends the policy and the revision of the row the person saw
    /// (`nil` when no summary was observed yet), with a new idempotency key (spec 0031).
    public var setStartup:
        @MainActor @Sendable (UUID, StartupPolicyKind, UInt64?) async -> MutationOutcome
    /// The session and the agent the Autostart row words its text with (spec 0031).
    public var autostartContext: @MainActor @Sendable () -> AutostartContext

    /// Creates a service.
    /// - Parameters:
    ///   - detail: Reads the detail of one instance.
    ///   - usage: Reads the data size of one instance with `refresh` and `cachedOnly`.
    ///   - instance: Reads one instance summary, or `nil` when it is gone.
    ///   - secretEvents: Opens the stream of `secret.` events.
    ///   - secrets: The secret methods.
    ///   - prepareTerminal: Writes the script of a `terminal` action and returns its path.
    ///   - setStartup: Sends `instance.set_startup` with the revision of the shown row.
    ///   - autostartContext: Reads the session and the agent for the Autostart text.
    public init(
        detail: @escaping @Sendable (UUID) async throws(ClientError) -> InstanceDetail,
        usage: @escaping @Sendable (UUID, Bool, Bool) async throws(ClientError) -> DataUsage?,
        instance: @escaping @Sendable (UUID) async throws(ClientError) -> InstanceSummary?,
        secretEvents: @escaping @MainActor @Sendable () -> AsyncStream<Event>,
        secrets: SecretsClient,
        prepareTerminal: @escaping @Sendable (UUID, String) async throws(ClientError) -> String =
            { _, _ throws(ClientError) in throw .notSent(ClientErrorCatalog.notConnected()) },
        setStartup:
            @escaping @MainActor @Sendable (UUID, StartupPolicyKind, UInt64?) async
            -> MutationOutcome = { _, _, _ in .notSent(ClientErrorCatalog.notConnected()) },
        autostartContext: @escaping @MainActor @Sendable () -> AutostartContext = { .unknown }
    ) {
        self.detail = detail
        self.usage = usage
        self.instance = instance
        self.secretEvents = secretEvents
        self.secrets = secrets
        self.prepareTerminal = prepareTerminal
        self.setStartup = setStartup
        self.autostartContext = autostartContext
    }

    /// A service over the detail methods of a client.
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
            secrets: model.secrets,
            prepareTerminal: { id, action throws(ClientError) in
                let params = PrepareTerminalParams(instanceId: id, action: action)
                let reply = try await model.read(.instancePrepareTerminal(params)).get()
                guard case .instancePrepareTerminal(let prepared) = reply else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return prepared.scriptPath
            },
            setStartup: { id, startup, revision in
                let call = Call.instanceSetStartup(
                    SetStartupParams(instanceId: id, startup: startup, expectedRevision: revision))
                return await MutationPath(model: model).perform(call)
            },
            autostartContext: {
                AutostartContext(
                    rotation: model.lastDaemon?.info?.session?.rotation,
                    agent: model.agent?.assessment?.state, rootOverride: model.rootOverrideActive)
            })
    }
}
