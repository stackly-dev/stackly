import Foundation

/// The waits of the background agent in the GUI (spec 0015, *Value sourcing*). The model
/// waits through the `sleep` the daemon model gives it and never compares clocks, because
/// `now` is fixed in fixtures.
nonisolated public enum AgentTiming {
    /// How long a daemon may be missing before the banner offers Start stacklyd.
    public static let startGrace: Duration = .seconds(15)
    /// How often the status is read again while the daemon is unreachable.
    public static let pollInterval: Duration = .seconds(5)
    /// How long Turn On and Start stacklyd wait for the daemon to answer.
    public static let startWait: Duration = .seconds(30)
}
