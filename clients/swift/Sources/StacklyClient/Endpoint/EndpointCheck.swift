import Darwin

/// What `lstat` reports about the socket path or its folder: what it is, its
/// owner, and its mode.
public struct FileStat: Hashable, Sendable {
    /// What a path is, without following a symlink.
    public enum Kind: Hashable, Sendable {
        /// A Unix domain socket.
        case socket
        /// A directory.
        case directory
        /// A symbolic link.
        case symlink
        /// Anything else: a regular file, a fifo, a device.
        case other
    }

    /// What the path is.
    public let kind: Kind
    /// The owner's uid.
    public let uid: uid_t
    /// The mode bits; only the permission bits are checked.
    public let mode: mode_t

    /// Facts about one path.
    public init(kind: Kind, uid: uid_t, mode: mode_t) {
        self.kind = kind
        self.uid = uid
        self.mode = mode
    }

    /// The facts of an `lstat` result.
    init(_ info: stat) {
        let kind: Kind =
            switch info.st_mode & S_IFMT {
            case S_IFSOCK: .socket
            case S_IFDIR: .directory
            case S_IFLNK: .symlink
            default: .other
            }
        self.init(kind: kind, uid: info.st_uid, mode: info.st_mode)
    }
}

/// The trust rules for the socket path and its folder, in the fixed order of
/// spec 0006, the same as Rust's `stackly_ipc::evaluate_path`. The shared
/// fixture `protocols/fixtures/endpoint/checks.json` keeps both in step.
public enum EndpointCheck {
    /// Permission bits for the group and for others.
    static let groupOrOther: mode_t = 0o077

    /// The first check the socket path fails: a symlink, not a socket,
    /// another owner, access for group or others. `nil` when it passes.
    public static func socketReason(_ path: FileStat, uid: uid_t) -> UntrustedReason? {
        switch path.kind {
        case .symlink: .symlink
        case .directory, .other: .notASocket
        case .socket where path.uid != uid: .wrongOwner
        case .socket where path.mode & groupOrOther != 0: .groupOrOtherAccess
        case .socket: nil
        }
    }

    /// The first check the socket's folder fails: a symlink, not a directory
    /// or another owner, access for group or others. `nil` when it passes.
    public static func folderReason(_ parent: FileStat, uid: uid_t) -> UntrustedReason? {
        switch parent.kind {
        case .symlink: .symlink
        case .socket, .other: .parentWrongOwner
        case .directory where parent.uid != uid: .parentWrongOwner
        case .directory where parent.mode & groupOrOther != 0: .parentGroupOrOtherAccess
        case .directory: nil
        }
    }

    /// The first check that fails: the socket's checks, then its folder's.
    /// `nil` when both pass.
    ///
    /// - Parameters:
    ///   - path: The socket path's facts.
    ///   - parent: The folder's facts.
    ///   - uid: The uid that must own both.
    public static func evaluate(path: FileStat, parent: FileStat, uid: uid_t) -> UntrustedReason? {
        socketReason(path, uid: uid) ?? folderReason(parent, uid: uid)
    }
}

/// Whose socket the client accepts. In production both uids are the
/// client's own; tests give another `peerUID` to exercise the peer check.
public struct ConnectPolicy: Hashable, Sendable {
    /// The uid that must own the socket and its folder.
    public let ownerUID: uid_t
    /// The uid the server process must run as.
    public let peerUID: uid_t

    /// A policy with separate uids for the path checks and the peer check.
    public init(ownerUID: uid_t, peerUID: uid_t) {
        self.ownerUID = ownerUID
        self.peerUID = peerUID
    }

    /// The production policy: the socket, its folder, and the server belong
    /// to this process's user.
    public static var sameUser: ConnectPolicy {
        let uid = getuid()
        return ConnectPolicy(ownerUID: uid, peerUID: uid)
    }
}
