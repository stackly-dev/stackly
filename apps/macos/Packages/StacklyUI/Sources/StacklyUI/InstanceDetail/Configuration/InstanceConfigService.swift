import Foundation
import StacklyClient

/// What the Configuration tab and the Rename and Ports sheets ask of the daemon (spec 0029,
/// *Method IPC*): the three reads, the mutation path with its idempotency key, and the
/// updates of an operation. The app builds one from its ``DaemonModel``; a preview, a snapshot,
/// or a test builds one that answers from data.
nonisolated public struct InstanceConfigService: Sendable {
    /// `instance.config.get`.
    public var get: @Sendable (UUID) async throws(ClientError) -> InstanceConfigView
    /// `instance.config.validate`, with the base revision and the change.
    public var validate:
        @Sendable (UUID, UInt64, ConfigChangeRequest) async throws(ClientError) -> ConfigValidation
    /// `instance.change_endpoints` with `dryRun`, with the base revision and the new ports.
    public var validateEndpoints:
        @Sendable (UUID, UInt64, [EndpointRequest]) async throws(ClientError) -> EndpointValidation
    /// Sends one mutation with its idempotency key.
    public var send: @MainActor @Sendable (Call, String) async -> MutationOutcome
    /// The updates of an operation, ending after its final state.
    public var updates: @MainActor @Sendable (UUID) -> AsyncStream<OperationSnapshot>
    /// One `operation.get`, or `nil` when it fails.
    public var readOperation: @MainActor @Sendable (UUID) async -> OperationSnapshot?

    /// Creates a service.
    /// - Parameters:
    ///   - get: Reads the configuration of one instance.
    ///   - validate: Judges a change without writing it.
    ///   - validateEndpoints: Judges new ports without writing them.
    ///   - send: Sends a mutation with its idempotency key.
    ///   - updates: Follows an operation.
    ///   - readOperation: Reads an operation once.
    public init(
        get: @escaping @Sendable (UUID) async throws(ClientError) -> InstanceConfigView,
        validate:
            @escaping @Sendable (UUID, UInt64, ConfigChangeRequest) async throws(ClientError)
            -> ConfigValidation,
        validateEndpoints:
            @escaping @Sendable (UUID, UInt64, [EndpointRequest])
            async throws(ClientError) -> EndpointValidation,
        send: @escaping @MainActor @Sendable (Call, String) async -> MutationOutcome,
        updates: @escaping @MainActor @Sendable (UUID) -> AsyncStream<OperationSnapshot>,
        readOperation: @escaping @MainActor @Sendable (UUID) async -> OperationSnapshot?
    ) {
        self.get = get
        self.validate = validate
        self.validateEndpoints = validateEndpoints
        self.send = send
        self.updates = updates
        self.readOperation = readOperation
    }

    /// A service for a window that has no daemon: every call fails as not connected.
    public static var disconnected: InstanceConfigService {
        InstanceConfigService(
            get: { _ throws(ClientError) in throw .notSent(ClientErrorCatalog.notConnected()) },
            validate: { _, _, _ throws(ClientError) in
                throw .notSent(ClientErrorCatalog.notConnected())
            },
            validateEndpoints: { _, _, _ throws(ClientError) in
                throw .notSent(ClientErrorCatalog.notConnected())
            },
            send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) },
            updates: { _ in AsyncStream { $0.finish() } },
            readOperation: { _ in nil })
    }
}

extension InstanceConfigService {
    /// The service of the running app: the reads go through the model's link and the
    /// mutations through its ``MutationPath``.
    /// - Parameter model: The daemon model of the window.
    static func live(_ model: DaemonModel) -> InstanceConfigService {
        let path = MutationPath(model: model)
        let operations = model.operations
        return InstanceConfigService(
            get: { id throws(ClientError) in
                let reply = try await model.read(
                    .instanceConfigGet(GetConfigParams(instanceId: id))
                )
                .get()
                guard case .instanceConfigGet(let view) = reply else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return view
            },
            validate: { id, base, change throws(ClientError) in
                let params = ValidateConfigParams(
                    instanceId: id, baseRevision: base, change: change)
                let reply = try await model.read(.instanceConfigValidate(params)).get()
                guard case .instanceConfigValidate(let validation) = reply else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return validation
            },
            validateEndpoints: { id, base, endpoints throws(ClientError) in
                let params = ChangeEndpointsParams(
                    instanceId: id, baseRevision: base, endpoints: endpoints, dryRun: true,
                    overrideLeases: nil)
                let reply = try await model.read(.instanceChangeEndpoints(params)).get()
                guard case .instanceChangeEndpoints(.dryRun(let validation)) = reply else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return validation
            },
            send: { call, key in await path.perform(call, idempotencyKey: key) },
            updates: { id in operations.updates(for: id) },
            readOperation: { id in
                do {
                    return try await OperationCenter.operation(
                        id,
                        call: { call throws(ClientError) in
                            try await model.read(call).get()
                        })
                } catch {
                    return nil
                }
            })
    }
}

/// How a mutation of the Configuration tab or of a sheet ended, once its operation (if any)
/// reached a final state.
public enum ConfigMutationEnd: Equatable {
    /// The daemon accepted it and the operation succeeded; `impact` is what it did.
    case succeeded(revision: UInt64, impact: ConfigImpact)
    /// The change equals what is already there; nothing ran.
    case unchanged
    /// The daemon refused it before an operation started, with the error.
    case refused(ErrorBody)
    /// The operation ended in failure; the error is its failure, when it has one.
    case failed(ErrorBody?, revision: UInt64)
    /// The operation was cancelled.
    case cancelled
    /// Nothing reached the daemon, or the answer was lost (`outcome` says which).
    case notCompleted(MutationOutcome)
}

/// Sends a mutation of spec 0029 and follows its operation to the end, the way
/// ``InstanceActionModel`` does for a start or a stop.
@MainActor
struct ConfigMutationRunner {
    let service: InstanceConfigService

    /// Sends `call` with `key` and waits for the operation it started.
    /// - Parameters:
    ///   - call: A mutation of spec 0029.
    ///   - key: The idempotency key of the user action.
    /// - Returns: How it ended.
    func run(_ call: Call, key: String) async -> ConfigMutationEnd {
        let outcome = await service.send(call, key)
        switch outcome {
        case .completed(let reply):
            return await follow(reply)
        case .rejected(let body):
            return .refused(body)
        case .notSent, .outcomeUnknown:
            return .notCompleted(outcome)
        }
    }

    private func follow(_ reply: Reply) async -> ConfigMutationEnd {
        let accepted: (OperationSummary?, unchanged: Bool, revision: UInt64, impact: ConfigImpact)
        switch reply {
        case .instanceConfigApply(let value):
            accepted = (value.accepted?.operation, value.unchanged, value.revision, value.impact)
        case .instanceChangeEndpoints(.applied(let value)):
            accepted = (value.accepted?.operation, value.unchanged, value.revision, value.impact)
        case .instanceRename(let value), .instanceConfigKeepRevision(let value):
            accepted = (value.accepted?.operation, value.alreadyInState, 0, .none)
        default:
            return .refused(ClientErrorCatalog.protocolViolation())
        }
        guard accepted.unchanged == false, let operation = accepted.0 else { return .unchanged }
        let final = await finish(operation)
        switch final.state {
        case .succeeded: return .succeeded(revision: accepted.revision, impact: accepted.impact)
        case .failed: return .failed(final.failure?.error, revision: accepted.revision)
        case .cancelled: return .cancelled
        case .queued, .running, .cancelRequested, .needsRecovery:
            return .failed(nil, revision: accepted.revision)
        }
    }

    private func finish(_ operation: OperationSummary) async -> OperationSummary {
        var last = operation
        if Self.isWaiting(operation) {
            for await update in service.updates(operation.id) {
                last = update
                if Self.isWaiting(update) == false { break }
            }
        }
        if Self.isWaiting(last), let read = await service.readOperation(operation.id) {
            last = read
        }
        return last
    }

    private static func isWaiting(_ operation: OperationSummary) -> Bool {
        OperationCenter.isActive(operation.state) && operation.state != .needsRecovery
    }
}
