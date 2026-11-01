import Foundation

/// One log follow on one connection (spec 0027, `instance.logs_follow`): the daemon's reply,
/// the stream of its `log` frames, and the way to end it on the connection that opened it.
/// The stream ends with a throw when that connection closes; a reader then follows again
/// from its own cursor on the next connection.
public struct LogFollowSession: Sendable {
    /// The reply: the subscription, the store, and the head when the follow began.
    public let following: LogsFollowing
    /// The frames of the follow, ``DecodedFrame/log(_:)`` only.
    public let frames: DaemonConnection.EventStream
    /// Sends `instance.logs_unfollow` on the connection of the follow and ends ``frames``.
    /// It never throws: a closed connection already ended the follow.
    public let unfollow: @Sendable () async -> Void

    /// Creates a follow from its parts.
    /// - Parameters:
    ///   - following: The reply of `instance.logs_follow`.
    ///   - frames: The stream of `log` frames.
    ///   - unfollow: Ends the follow on its connection.
    public init(
        following: LogsFollowing, frames: DaemonConnection.EventStream,
        unfollow: @escaping @Sendable () async -> Void
    ) {
        self.following = following
        self.frames = frames
        self.unfollow = unfollow
    }
}
