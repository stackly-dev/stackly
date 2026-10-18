import Darwin

/// The `details.kind` of `ipc.io_error` for an errno: Rust's `io::ErrorKind`
/// for that errno in snake case, so the GUI and the CLI name OS errors alike.
/// The shared fixture `protocols/fixtures/endpoint/checks.json` lists every
/// errno here, and Rust checks the same table.
public enum ErrnoKind {
    /// The kind Rust gives an errno it has no kind for.
    public static let fallback = "uncategorized"

    /// The kind name of `errno`, such as `permission_denied`.
    ///
    /// - Parameter errno: The errno value.
    public static func name(of errno: Int32) -> String {
        switch errno {
        case EPERM: "permission_denied"
        case ENOENT: "not_found"
        case EINTR: "interrupted"
        case EIO: "input_output_error"
        case E2BIG: "argument_list_too_long"
        case EDEADLK: "deadlock"
        case ENOMEM: "out_of_memory"
        case EACCES: "permission_denied"
        case EBUSY: "resource_busy"
        case EEXIST: "already_exists"
        case EXDEV: "crosses_devices"
        case ENOTDIR: "not_a_directory"
        case EISDIR: "is_a_directory"
        case EINVAL: "invalid_input"
        case ENFILE: "too_many_open_files"
        case EMFILE: "too_many_open_files"
        case ETXTBSY: "executable_file_busy"
        case EFBIG: "file_too_large"
        case ENOSPC: "storage_full"
        case ESPIPE: "not_seekable"
        case EROFS: "read_only_filesystem"
        case EMLINK: "too_many_links"
        case EPIPE: "broken_pipe"
        case EAGAIN: "would_block"
        case EINPROGRESS: "in_progress"
        case EADDRINUSE: "addr_in_use"
        case EADDRNOTAVAIL: "addr_not_available"
        case ENETDOWN: "network_down"
        case ENETUNREACH: "network_unreachable"
        case ECONNABORTED: "connection_aborted"
        case ECONNRESET: "connection_reset"
        case ENOTCONN: "not_connected"
        case ETIMEDOUT: "timed_out"
        case ECONNREFUSED: "connection_refused"
        case ELOOP: "filesystem_loop"
        case ENAMETOOLONG: "invalid_filename"
        case EHOSTUNREACH: "host_unreachable"
        case ENOTEMPTY: "directory_not_empty"
        case EDQUOT: "quota_exceeded"
        case ESTALE: "stale_network_file_handle"
        case ENOSYS: "unsupported"
        case EOPNOTSUPP: "unsupported"
        default: fallback
        }
    }
}
