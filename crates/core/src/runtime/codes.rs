//! The catalog codes of the `runtime` area that core fails an operation
//! with (spec 0018, *Kode error*). `stacklyd` looks each one up in the
//! catalog of `stackly-protocol`, where the codes enter with step 10 of the
//! spec.

use stackly_domain::ErrorCodeName;

macro_rules! codes {
    ($($(#[$doc:meta])* $name:ident = $code:literal;)*) => {
        $($(#[$doc])* pub const $name: ErrorCodeName = ErrorCodeName::from_static($code);)*

        /// Every code above, for the catalog test of `stackly-protocol`.
        pub const ALL: &[ErrorCodeName] = &[$($name),*];
    };
}

codes! {
    /// The selector matched nothing.
    SELECTOR_UNRESOLVED = "runtime.selector_unresolved";
    /// The selector matched more than one version.
    SELECTOR_AMBIGUOUS = "runtime.selector_ambiguous";
    /// `expectedSha256` differs from the artifact the selector resolved to.
    SELECTOR_CHANGED = "runtime.selector_changed";
    /// A runtime for the artifact exists in a state other than `installed`.
    ALREADY_REGISTERED = "runtime.already_registered";
    /// No network and no cache entry.
    DOWNLOAD_OFFLINE = "runtime.download_offline";
    /// An HTTP or connection failure.
    DOWNLOAD_FAILED = "runtime.download_failed";
    /// TLS failed or a redirect left `https`.
    DOWNLOAD_UNTRUSTED = "runtime.download_untrusted";
    /// The length disagrees with the catalog, or the body is longer.
    DOWNLOAD_TOO_LARGE = "runtime.download_too_large";
    /// The SHA-256 of the artifact is not the catalog's.
    DIGEST_MISMATCH = "runtime.digest_mismatch";
    /// The size of the artifact is not the catalog's.
    SIZE_MISMATCH = "runtime.size_mismatch";
    /// The magic number does not match the format.
    FORMAT_MISMATCH = "runtime.format_mismatch";
    /// Not enough room for the download or the extraction.
    INSUFFICIENT_SPACE = "runtime.insufficient_space";
    /// Another I/O failure of a platform port.
    IO_FAILED = "runtime.io_failed";
    /// The archive breaks an extraction rule.
    ARCHIVE_REJECTED = "runtime.archive_rejected";
    /// No arm64 slice, or `EBADARCH` at spawn.
    ARCHITECTURE_MISMATCH = "runtime.architecture_mismatch";
    /// A binary needs a newer macOS than this Mac runs.
    OS_TOO_OLD = "runtime.os_too_old";
    /// An `executables` path is missing or not a regular file.
    EXECUTABLE_MISSING = "runtime.executable_missing";
    /// A binary loads a library outside the runtime.
    DEPENDENCY_OUTSIDE_RUNTIME = "runtime.dependency_outside_runtime";
    /// The libraries found differ from the catalog's dependencies.
    DEPENDENCY_MISMATCH = "runtime.dependency_mismatch";
    /// The signing policy is not met.
    SIGNATURE_INVALID = "runtime.signature_invalid";
    /// An executable carries the quarantine attribute.
    QUARANTINED = "runtime.quarantined";
    /// macOS killed the binary with `SIGKILL`.
    BINARY_REJECTED_BY_OS = "runtime.binary_rejected_by_os";
    /// The version check did not finish in time.
    BINARY_CHECK_TIMEOUT = "runtime.binary_check_timeout";
    /// The version check exited non zero, died of another signal, or could
    /// not start.
    BINARY_CHECK_FAILED = "runtime.binary_check_failed";
    /// The version the binary reports is not the catalog's.
    VERSION_MISMATCH = "runtime.version_mismatch";
    /// The target folder belongs to something else.
    INSTALL_CONFLICT = "runtime.install_conflict";
}
