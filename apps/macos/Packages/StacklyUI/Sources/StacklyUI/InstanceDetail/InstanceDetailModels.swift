import Foundation
import StacklyClient

/// The detail models of the Instances page, one per instance (spec 0021): the detail page,
/// the inspector, and the Copy Connection String command of one instance share the detail
/// and the data size the daemon last sent, so they never disagree and ask the daemon once.
/// The page owns one registry. A model goes away with it.
@MainActor
public final class InstanceDetailModels {
    private let service: InstanceDetailService
    private let reload: Duration
    private let sleep: @Sendable (Duration) async throws -> Void
    private var models: [UUID: InstanceDetailModel] = [:]

    /// The service the models read from and the Show Password and Copy buttons reveal with.
    public var detailService: InstanceDetailService { service }

    /// Creates an empty registry.
    /// - Parameters:
    ///   - service: The reads, the event stream, and the secret methods.
    ///   - reload: The pause before a reload after a revision change.
    ///   - sleep: Waits for a duration and throws when cancelled.
    public init(
        service: InstanceDetailService,
        reload: Duration = InstanceDetailModel.reloadDebounce,

        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.service = service
        self.reload = reload
        self.sleep = sleep
    }

    /// The model of an instance, made on first use.
    /// - Parameter id: The instance.
    public func model(for id: UUID) -> InstanceDetailModel {
        if let held = models[id] { return held }
        let made = InstanceDetailModel(
            instanceId: id, service: service, debounce: reload, sleep: sleep)
        models[id] = made
        return made
    }

    /// Drops the models of instances that are not in `ids`: they are gone from the store.
    /// - Parameter ids: The instances that exist.
    public func keep(_ ids: Set<UUID>) {
        for (id, model) in models where ids.contains(id) == false {
            model.close()
            models[id] = nil
        }
    }
}

extension InstanceDetailService {
    /// A service for a window that has no daemon (a preview of the page alone): every read
    /// fails as not connected, so the page shows its failed state.
    public static var disconnected: InstanceDetailService {
        InstanceDetailService(
            detail: { _ throws(ClientError) in throw .notSent(ClientErrorCatalog.notConnected()) },
            usage: { _, _, _ throws(ClientError) in
                throw .notSent(ClientErrorCatalog.notConnected())
            },
            instance: { _ throws(ClientError) in throw .notSent(ClientErrorCatalog.notConnected())
            },
            secretEvents: { AsyncStream { $0.finish() } },
            secrets: SecretsClient { _, _ throws(ClientError) in
                throw .notSent(ClientErrorCatalog.notConnected())
            })
    }
}
