/// How a call or a connection attempt failed, by what the daemon may have
/// done. Every case carries a catalog error body.
public enum ClientError: Error, Equatable, Sendable {
    /// Nothing reached the daemon, so the call had no effect.
    case notSent(ErrorBody)
    /// The daemon answered with an error frame.
    case rejected(ErrorBody)
    /// The request was written but no answer came, so it may have run.
    case outcomeUnknown(ErrorBody)

    /// The error body.
    public var body: ErrorBody {
        switch self {
        case .notSent(let body), .rejected(let body), .outcomeUnknown(let body): body
        }
    }
}
