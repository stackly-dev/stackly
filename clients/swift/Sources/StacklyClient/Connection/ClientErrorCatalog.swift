/// The error bodies the client makes itself, with the code, category,
/// `retryable`, and `details` keys of `protocols/error-codes.json` (spec 0013).
public enum ClientErrorCatalog {
    /// One client side code as the catalog defines it.
    struct Entry: Sendable {
        let code: String
        let category: ErrorCategory
        let retryable: Bool
        /// The `details` keys the code allows.
        let details: [String]
        /// The fixed message: lowercase, no trailing period.
        let message: String
    }

    /// The catalog entry of each client side code.
    enum Spec {
        static let endpointMissing = Entry(
            code: "ipc.endpoint_missing", category: .unavailable, retryable: true,
            details: ["path"], message: "no socket at the daemon path")
        static let endpointUntrusted = Entry(
            code: "ipc.endpoint_untrusted", category: .unauthorized, retryable: false,
            details: ["path", "reason"], message: "the daemon socket failed a security check")
        static let connectionRefused = Entry(
            code: "ipc.connection_refused", category: .unavailable, retryable: true,
            details: ["path"], message: "the daemon refused the connection")
        static let connectionClosed = Entry(
            code: "ipc.connection_closed", category: .unavailable, retryable: false,
            details: [], message: "the daemon closed the connection")
        static let ioError = Entry(
            code: "ipc.io_error", category: .unavailable, retryable: true,
            details: ["op", "kind"], message: "an os error stopped the connection")
        static let protocolViolation = Entry(
            code: "ipc.protocol_violation", category: .internal, retryable: false,
            details: [], message: "the daemon sent a frame the client could not read")
        static let frameEmpty = Entry(
            code: "ipc.frame_empty", category: .invalidInput, retryable: false,
            details: [], message: "the daemon sent an empty frame")
        static let frameTooLarge = Entry(
            code: "ipc.frame_too_large", category: .invalidInput, retryable: false,
            details: ["length", "max"], message: "the daemon sent a frame above the size limit")
        static let handshakeTimeout = Entry(
            code: "ipc.handshake_timeout", category: .timeout, retryable: false,
            details: [], message: "the daemon did not answer the handshake in time")
        static let notConnected = Entry(
            code: "ipc.not_connected", category: .unavailable, retryable: true,
            details: [], message: "the client is not connected to the daemon")
        static let callTimeout = Entry(
            code: "ipc.call_timeout", category: .timeout, retryable: false,
            details: ["method", "seconds", "idempotencyKey"],
            message: "the daemon did not answer in time")
    }

    /// Every client side code, for the contract test.
    static let entries = [
        Spec.endpointMissing, Spec.endpointUntrusted, Spec.connectionRefused,
        Spec.connectionClosed, Spec.ioError, Spec.protocolViolation, Spec.frameEmpty,
        Spec.frameTooLarge, Spec.handshakeTimeout, Spec.notConnected, Spec.callTimeout,
    ]

    /// `ipc.endpoint_missing` for the socket at `path`.
    public static func endpointMissing(path: String) -> ErrorBody {
        body(Spec.endpointMissing, ["path": path])
    }

    /// `ipc.endpoint_untrusted` with the failed check.
    public static func endpointUntrusted(path: String?, reason: UntrustedReason) -> ErrorBody {
        var details = ["reason": reason.rawValue]
        details["path"] = path
        return body(Spec.endpointUntrusted, details)
    }

    /// `ipc.connection_refused` for a stale socket at `path`.
    public static func connectionRefused(path: String) -> ErrorBody {
        body(Spec.connectionRefused, ["path": path])
    }

    /// `ipc.connection_closed`.
    public static func connectionClosed() -> ErrorBody {
        body(Spec.connectionClosed, [:])
    }

    /// `ipc.io_error` for `op` (`stat`, `connect`, `read`, `write`) that
    /// failed with `errno`.
    public static func ioError(op: String, errno: Int32) -> ErrorBody {
        body(Spec.ioError, ["op": op, "kind": ErrnoKind.name(of: errno)])
    }

    /// `ipc.protocol_violation`.
    public static func protocolViolation() -> ErrorBody {
        body(Spec.protocolViolation, [:])
    }

    /// `ipc.frame_empty` or `ipc.frame_too_large` for a frame the daemon sent.
    public static func frame(_ error: FrameError) -> ErrorBody {
        switch error {
        case .empty:
            body(Spec.frameEmpty, [:])
        case .tooLarge(let length, let max):
            body(Spec.frameTooLarge, ["length": String(length), "max": String(max)])
        }
    }

    /// `ipc.handshake_timeout`.
    public static func handshakeTimeout() -> ErrorBody {
        body(Spec.handshakeTimeout, [:])
    }

    /// `ipc.not_connected`: the client is not connected and sent nothing.
    public static func notConnected() -> ErrorBody {
        body(Spec.notConnected, [:])
    }

    /// `ipc.call_timeout` for `method`: the client stopped waiting after
    /// `limit`, written as whole seconds. A mutation names its
    /// `idempotencyKey`, so the caller can send the same request again
    /// safely; the request may already have run.
    public static func callTimeout(
        method: String, limit: Duration, idempotencyKey: String? = nil
    ) -> ErrorBody {
        var details = ["method": method, "seconds": String(limit.components.seconds)]
        details["idempotencyKey"] = idempotencyKey
        return body(Spec.callTimeout, details)
    }

    /// The code of the body ``frame(_:)`` makes.
    public static func code(of error: FrameError) -> String {
        frame(error).code
    }

    /// The body of `entry` with `details` in the catalog's key order, and the
    /// entry's message unless `message` replaces it.
    static func body(
        _ entry: Entry,
        _ details: [String: String],
        message: String? = nil
    ) -> ErrorBody {
        assert(Set(details.keys).isSubset(of: entry.details), "keys outside the catalog entry")
        let ordered = entry.details.compactMap { key in
            details[key].map { ErrorDetail(key: key, value: $0) }
        }
        return ErrorBody(
            code: entry.code,
            category: entry.category,
            message: message ?? entry.message,
            retryable: entry.retryable,
            resource: nil,
            fields: nil,
            hint: nil,
            details: ordered.isEmpty ? nil : ordered,
            operationId: nil,
            rule: nil,
            items: nil
        )
    }
}

extension ErrorBody {
    /// The value of `details[key]`, or `nil`.
    public func detail(_ key: String) -> String? {
        details?.first { $0.key == key }?.value
    }
}
