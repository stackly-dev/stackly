/// The limits the client uses before the handshake gives the daemon's own
/// (spec 0006), plus the client's own time limits (spec 0013).
public enum ContractLimits {
    /// The largest control frame payload before the handshake: 1 MiB.
    public static let controlFrameMax: UInt32 = 1_048_576
    /// The deepest JSON nesting accepted before the handshake.
    public static let maxJsonDepth: UInt32 = 32
    /// The most bytes read from the socket at once.
    public static let readChunk = 65_536
    /// The limit for connecting plus the handshake.
    public static let handshakeTimeout: Duration = .seconds(5)
    /// The default limit for one call, including the wait for a request slot.
    public static let callTimeout: Duration = .seconds(10)
    /// How long the client still reads after a failed write, for the daemon's
    /// error frame.
    public static let pendingReadTimeout: Duration = .seconds(1)
}
