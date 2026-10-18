/// Why the client did not trust the socket: the fixed `details.reason` values
/// of `ipc.endpoint_untrusted` (spec 0006).
public enum UntrustedReason: String, CaseIterable, Sendable {
    /// The path is not a socket.
    case notASocket = "not_a_socket"
    /// The path or its folder is a symbolic link.
    case symlink
    /// Another user owns the socket.
    case wrongOwner = "wrong_owner"
    /// The socket grants access to its group or to others.
    case groupOrOtherAccess = "group_or_other_access"
    /// The folder is not a folder, or another user owns it.
    case parentWrongOwner = "parent_wrong_owner"
    /// The folder grants access to its group or to others.
    case parentGroupOrOtherAccess = "parent_group_or_other_access"
    /// The server process runs as another user.
    case peerUidMismatch = "peer_uid_mismatch"
    /// The server process's credentials cannot be read.
    case peerCredentialsUnavailable = "peer_credentials_unavailable"
}
