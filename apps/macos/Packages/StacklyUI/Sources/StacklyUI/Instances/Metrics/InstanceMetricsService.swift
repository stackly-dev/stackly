import Foundation
import StacklyClient

/// What the Metrics tab asks of the daemon (spec 0028, *Layar Metrics*): `instance.get_metrics`
/// with the cursor of the history the tab holds. The app builds one from its ``DaemonModel``;
/// a preview, a snapshot, or a test builds one that answers from data.
nonisolated public struct InstanceMetricsService: Sendable {
    /// `instance.get_metrics`, with the `historyId` and the newest `seq` the caller holds.
    public var read: @Sendable (UUID, String?, UInt64?) async throws(ClientError) -> MetricsReply

    /// Creates a service.
    /// - Parameter read: Reads the samples after a cursor.
    public init(
        read: @escaping @Sendable (UUID, String?, UInt64?) async throws(ClientError) -> MetricsReply
    ) {
        self.read = read
    }

    /// A service for a window that has no daemon: every call fails as not connected.
    public static var disconnected: InstanceMetricsService {
        InstanceMetricsService { _, _, _ throws(ClientError) in
            throw .notSent(ClientErrorCatalog.notConnected())
        }
    }
}

extension InstanceMetricsService {
    /// The service of the running app: the call goes through the model's link.
    /// - Parameter model: The daemon model of the window.
    static func live(_ model: DaemonModel) -> InstanceMetricsService {
        InstanceMetricsService { id, historyId, afterSeq throws(ClientError) in
            let params = GetMetricsParams(instanceId: id, historyId: historyId, afterSeq: afterSeq)
            let reply = try await model.read(.instanceGetMetrics(params)).get()
            guard case .instanceGetMetrics(let metrics) = reply else {
                throw .rejected(ClientErrorCatalog.protocolViolation())
            }
            return metrics
        }
    }
}
