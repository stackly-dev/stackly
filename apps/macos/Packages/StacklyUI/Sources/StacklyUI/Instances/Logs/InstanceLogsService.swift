import Foundation
import StacklyClient

/// What the Logs tab asks of the daemon (spec 0027, *Layar Logs*): `instance.logs_read` for a
/// page and `instance.logs_follow` for the frames after a cursor. The app builds one from its
/// ``DaemonModel``; a preview, a snapshot, or a test builds one that answers from data.
nonisolated public struct InstanceLogsService: Sendable {
    /// `instance.logs_read`.
    public var read: @Sendable (LogsReadParams) async throws(ClientError) -> LogPage
    /// `instance.logs_follow`, with its frames and its unfollow.
    public var follow: @Sendable (LogsFollowParams) async throws(ClientError) -> LogFollowSession

    /// Creates a service.
    /// - Parameters:
    ///   - read: Reads one page.
    ///   - follow: Follows the entries after a cursor.
    public init(
        read: @escaping @Sendable (LogsReadParams) async throws(ClientError) -> LogPage,
        follow: @escaping @Sendable (LogsFollowParams) async throws(ClientError) -> LogFollowSession
    ) {
        self.read = read
        self.follow = follow
    }

    /// A service for a window that has no daemon: every call fails as not connected.
    public static var disconnected: InstanceLogsService {
        InstanceLogsService(
            read: { _ throws(ClientError) in throw .notSent(ClientErrorCatalog.notConnected()) },
            follow: { _ throws(ClientError) in throw .notSent(ClientErrorCatalog.notConnected()) })
    }
}

extension InstanceLogsService {
    /// The service of the running app: both calls go through the model's link.
    /// - Parameter model: The daemon model of the window.
    static func live(_ model: DaemonModel) -> InstanceLogsService {
        InstanceLogsService(
            read: { params throws(ClientError) in
                let reply = try await model.read(.instanceLogsRead(params)).get()
                guard case .instanceLogsRead(let page) = reply else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return page
            },
            follow: { params throws(ClientError) in try await model.followLogs(params) })
    }
}
