//! The error code catalog, one submodule per area (VAL-020).
//!
//! Every code on the wire is `<area>.<reason>` in snake case and lives here
//! with its category, its `retryable` flag, the `details` keys it may
//! carry, and the `items` it may list. Each spec owns its areas and adds codes only there, so specs
//! written in parallel never edit the same list. The spec that first needs a
//! new area adds it to [`AREAS`] with the `area!` macro. `cargo xtask
//! codegen` writes the catalog to `protocols/error-codes.json`.

use crate::error::ErrorCategory;

/// One catalog entry: everything about a code that never depends on where
/// the error happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ErrorSpec {
    /// The code, `<area>.<reason>`.
    pub code: &'static str,
    /// The category every error with this code carries.
    pub category: ErrorCategory,
    /// True only when an error with this code guarantees the request had no
    /// effect.
    pub retryable: bool,
    /// The camelCase `details` keys an error with this code may carry.
    pub details: &'static [&'static str],
    /// The list items an error with this code may carry, or `None` when it
    /// carries none.
    pub items: Option<ItemSpec>,
    /// When the code is used, in one English sentence.
    pub summary: &'static str,
}

/// The list items one code allows: one item kind, its field keys, and how
/// many items an error carries at most. A code with items always lists the
/// `omitted` detail, which counts the items dropped above `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ItemSpec {
    /// The snake_case item kind, such as `issue` (spec 0009) or
    /// `reference` (spec 0011).
    pub kind: &'static str,
    /// The camelCase field keys an item may carry.
    pub fields: &'static [&'static str],
    /// The most items one error carries.
    pub max: u16,
}

/// One registered area and its codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Area {
    /// The area name, the part of each code before the dot.
    pub name: &'static str,
    /// The spec that owns the area.
    pub owner: &'static str,
    /// Every code in the area.
    pub codes: &'static [ErrorSpec],
}

/// Defines one area submodule: a constant per code and the `ALL` slice. A
/// code that carries list items adds
/// `items { kind "<kind>", fields [..], max <n> }` after its details. An
/// area with hints adds `@functions` and its `hint` function after its codes
/// (spec 0006, *Hint*).
macro_rules! area {
    (@items) => { None };
    (@items $kind:literal, [$($field:literal),* $(,)?], $max:literal) => {
        Some(super::ItemSpec { kind: $kind, fields: &[$($field),*], max: $max })
    };
    (
        $(#[$meta:meta])*
        pub mod $module:ident = $area:literal, owner $owner:literal {
            $(
                $konst:ident = $code:literal, $category:ident, retryable $retryable:literal,
                details [$($key:literal),* $(,)?],
                $(items { kind $item_kind:literal, fields [$($item_field:literal),* $(,)?], max $item_max:literal },)?
                $summary:literal;
            )*
            $(@functions $($extra:item)*)?
        }
    ) => {
        $(#[$meta])*
        pub mod $module {
            use super::{Area, ErrorCategory, ErrorSpec};

            /// The area name.
            pub const NAME: &str = $area;

            $(
                #[doc = concat!("`", $code, "`: ", $summary, ".")]
                pub const $konst: ErrorSpec = ErrorSpec {
                    code: $code,
                    category: ErrorCategory::$category,
                    retryable: $retryable,
                    details: &[$($key),*],
                    items: area!(@items $($item_kind, [$($item_field),*], $item_max)?),
                    summary: $summary,
                };
            )*

            /// Every code in this area.
            pub const ALL: &[ErrorSpec] = &[$($konst),*];

            /// This area, as registered in [`AREAS`](super::AREAS).
            pub const AREA: Area = Area { name: NAME, owner: $owner, codes: ALL };

            $($($extra)*)?
        }
    };
}

area! {
    /// Version and handshake codes.
    pub mod protocol = "protocol", owner "spec 0006" {
        HANDSHAKE_REQUIRED = "protocol.handshake_required", InvalidInput, retryable false,
            details ["method"], "the first request on a connection is valid but not a handshake";
        HANDSHAKE_REPEATED = "protocol.handshake_repeated", InvalidInput, retryable false,
            details [], "a second handshake arrived on the same connection";
        MAJOR_MISMATCH = "protocol.major_mismatch", Unsupported, retryable false,
            details ["serverMajor", "serverMinor"], "the client speaks another protocol major";
    }
}

area! {
    /// Socket, framing, and endpoint trust codes.
    pub mod ipc = "ipc", owner "spec 0006" {
        PEER_REJECTED = "ipc.peer_rejected", Unauthorized, retryable false,
            details [], "the peer runs as another user or its credentials cannot be read";
        ENDPOINT_MISSING = "ipc.endpoint_missing", Unavailable, retryable true,
            details ["path"], "client side: nothing exists at the socket path";
        ENDPOINT_UNTRUSTED = "ipc.endpoint_untrusted", Unauthorized, retryable false,
            details ["path", "reason"], "client side: the socket path or the server peer failed a trust check";
        CONNECTION_REFUSED = "ipc.connection_refused", Unavailable, retryable true,
            details ["path"], "client side: connecting was refused, the socket is stale";
        CONNECTION_CLOSED = "ipc.connection_closed", Unavailable, retryable false,
            details [], "client side: the daemon closed the connection, or reading or writing it failed, before the reply arrived, so the request may have run";
        IO_ERROR = "ipc.io_error", Unavailable, retryable true,
            details ["op", "kind"], "another os error before any request was sent: checking the socket path or connecting (client side), or binding, securing, or accepting on the socket (daemon side)";
        PROTOCOL_VIOLATION = "ipc.protocol_violation", Internal, retryable false,
            details [], "client side: a daemon frame cannot be decoded or its reply tag does not match the call";
        ENDPOINT_OCCUPIED = "ipc.endpoint_occupied", Conflict, retryable false,
            details ["path", "kind"], "daemon side: the socket path holds something that is not a socket";
        FRAME_EMPTY = "ipc.frame_empty", InvalidInput, retryable false,
            details [], "a frame header announces a length of 0";
        FRAME_TOO_LARGE = "ipc.frame_too_large", InvalidInput, retryable false,
            details ["length", "max"], "a frame header announces a length above the limit";
        FRAME_STALLED = "ipc.frame_stalled", Timeout, retryable false,
            details [], "a started frame was still incomplete after 30 seconds";
        HANDSHAKE_TIMEOUT = "ipc.handshake_timeout", Timeout, retryable false,
            details [], "the handshake frame was not complete within 5 seconds";
        WRITE_TIMEOUT = "ipc.write_timeout", Timeout, retryable false,
            details [], "daemon side: the client read nothing for 30 seconds, only ever logged";
        REPLY_TOO_LARGE = "ipc.reply_too_large", Internal, retryable false,
            details ["method", "length"], "daemon side: a reply is larger than a frame and is sent instead of it";
        TOO_MANY_CONNECTIONS = "ipc.too_many_connections", Unavailable, retryable true,
            details ["max"], "the daemon already serves the most connections it allows";
        NOT_CONNECTED = "ipc.not_connected", Unavailable, retryable true,
            details [], "client side: the client is not connected to the daemon, so it sent nothing";
        CALL_TIMEOUT = "ipc.call_timeout", Timeout, retryable false,
            details ["method", "seconds", "idempotencyKey"], "client side: the client stopped waiting for a reply, so the request may have run";
    }
}

area! {
    /// Request decoding and request bookkeeping codes.
    pub mod request = "request", owner "spec 0006" {
        MALFORMED = "request.malformed", InvalidInput, retryable false,
            details ["omitted"], "the json is broken, the request or its call is not a json object, or requestId or idempotencyKey has a bad format";
        TOO_DEEP = "request.too_deep", InvalidInput, retryable false,
            details ["max"], "the json nests deeper than the limit";
        UNKNOWN_METHOD = "request.unknown_method", Unsupported, retryable false,
            details ["method"], "the daemon does not know the method";
        INVALID_PARAMS = "request.invalid_params", InvalidInput, retryable false,
            details ["method", "omitted"], "the params are not a json object or fail to decode or to validate; fields lists the first 32 failures and omitted counts the rest";
        DUPLICATE_ID = "request.duplicate_id", InvalidInput, retryable false,
            details [], "another request with this requestId is still running on the connection";
        TOO_MANY_IN_FLIGHT = "request.too_many_in_flight", Unavailable, retryable true,
            details ["max"], "the connection already has the most requests in flight it allows";
    }
}

area! {
    /// Event subscription codes.
    pub mod events = "events", owner "spec 0006" {
        TOO_MANY_SUBSCRIPTIONS = "events.too_many_subscriptions", InvalidInput, retryable false,
            details ["max"], "the connection already holds the most subscriptions it allows";
        UNKNOWN_SUBSCRIPTION = "events.unknown_subscription", NotFound, retryable false,
            details ["subscriptionId"], "no subscription with this id exists on the connection";
    }
}

area! {
    /// Codes shared by every resource owner.
    pub mod resource = "resource", owner "spec 0006" {
        NOT_FOUND = "resource.not_found", NotFound, retryable false,
            details [], "the resource does not exist, reserved for resource owners";
        REVISION_MISMATCH = "resource.revision_mismatch", Conflict, retryable false,
            details ["expected", "actual"], "the resource revision is not the expected one, reserved for the registry compare and swap";
        BUSY = "resource.busy", Conflict, retryable true,
            details ["operationId"], "another operation holds the resource lock, reserved for the operation engine";
    }
}

area! {
    /// Daemon lifecycle codes.
    pub mod daemon = "daemon", owner "spec 0006" {
        STARTING = "daemon.starting", Unavailable, retryable true,
            details ["subsystem"], "the method needs a subsystem that is not ready while the daemon starts";
        SHUTTING_DOWN = "daemon.shutting_down", Unavailable, retryable true,
            details [], "the request arrived while the daemon stops";
        RESTART_UNAVAILABLE = "daemon.restart_unavailable", Unsupported, retryable false,
            details ["launchContext"], "a restart was asked of a daemon that launchd did not start, so nothing would start it again";
        LOCK_HELD = "daemon.lock_held", Conflict, retryable false,
            details ["pid", "serverInstanceId", "version", "startedAt"], "another daemon holds the lock, only ever printed to stderr";
    }
}

area! {
    /// Registry codes (spec 0005, *Kode error di wire*): one per
    /// `RegistryError` variant that has no spec 0006 code, plus recovery
    /// mode. None is retryable: a registry failure in the middle of a
    /// request never promises the request had no effect.
    pub mod registry = "registry", owner "spec 0005" {
        MISSING_REFERENCE = "registry.missing_reference", Conflict, retryable false,
            details [], "a write refers to a resource that does not exist";
        DUPLICATE_RESOURCE = "registry.duplicate_resource", Conflict, retryable false,
            details [], "an insert names a resource that already exists";
        GENERATION_CONFLICT = "registry.generation_conflict", Conflict, retryable false,
            details ["expected", "actual"], "the registry changed since the caller read it, after the caller's own retries";
        NAME_TAKEN = "registry.name_taken", Conflict, retryable false,
            details ["name", "existing"], "another instance has the instance name";
        SHORT_KEY_TAKEN = "registry.short_key_taken", Conflict, retryable false,
            details ["key", "existing"], "another instance has the short key";
        PATH_TAKEN = "registry.path_taken", Conflict, retryable false,
            details ["root", "path", "existing"], "another instance stores the same path under the same root";
        DUPLICATE_RUNTIME = "registry.duplicate_runtime", Conflict, retryable false,
            details ["existing"], "a runtime for the same managed artifact or external folder exists";
        DEDICATED_OWNER_EXISTS = "registry.dedicated_owner_exists", Conflict, retryable false,
            details ["instance", "environment"], "another environment already owns the instance as a dedicated member";
        ACTIVE_LEASE_EXISTS = "registry.active_lease_exists", Conflict, retryable false,
            details ["instance", "environment"], "the environment already holds an active lease on the instance";
        SOFTWARE_MISMATCH = "registry.software_mismatch", InvalidInput, retryable false,
            details ["instance", "runtime", "providerRevision"], "the instance software differs from its runtime or provider revision software";
        PATH_NOT_UTF8 = "registry.path_not_utf8", InvalidInput, retryable false,
            details ["field"], "a path to store is not valid utf-8";
        BUSY = "registry.busy", Unavailable, retryable false,
            details [], "the registry stayed locked past its busy timeout";
        UNAVAILABLE = "registry.unavailable", Unavailable, retryable false,
            details [], "the registry cannot serve calls any more";
        NO_SPACE = "registry.no_space", Failed, retryable false,
            details [], "the disk holding the registry is full";
        IO_ERROR = "registry.io_error", Failed, retryable false,
            details [], "another i/o failure while using the registry";
        CORRUPT = "registry.corrupt", Internal, retryable false,
            details ["table", "column"], "a stored registry value cannot be read";
        INTEGRITY = "registry.integrity", Internal, retryable false,
            details [], "a database constraint refused a write that an earlier check should have caught";
        RECOVERY_MODE = "registry.recovery_mode", Unavailable, retryable false,
            details ["reason", "reportPath"], "the registry is in recovery mode, so the daemon only answers the methods allowed there";
        NOT_DELETABLE = "registry.not_deletable", Conflict, retryable false,
            details ["reason"], "an operation tried to remove an instance it did not create, or one that is no longer new";
    }
}

area! {
    /// Operation engine codes (spec 0007, *Kode error*). Only
    /// `operation.queue_full` is retryable. `interrupted`,
    /// `daemon_stopping`, `abandoned`, and `compensation_failed` are only
    /// ever the final error of an operation, in its `failure.error`.
    pub mod operation = "operation", owner "spec 0007" {
        IDEMPOTENCY_KEY_REQUIRED = "operation.idempotency_key_required", InvalidInput, retryable false,
            details ["method"], "a method that starts an operation arrived without an idempotency key";
        IDEMPOTENCY_KEY_REUSED = "operation.idempotency_key_reused", InvalidInput, retryable false,
            details ["operationId", "method"], "the idempotency key belongs to an operation another request started";
        QUEUE_FULL = "operation.queue_full", Unavailable, retryable true,
            details ["max"], "the operation queue is full";
        RECOVERY_REQUIRED = "operation.recovery_required", Conflict, retryable false,
            details ["operationId", "lockKey"], "an operation that needs recovery holds a lock the request needs";
        NOT_RESUMABLE = "operation.not_resumable", Conflict, retryable false,
            details ["operationId", "reason"], "the operation cannot be resumed";
        NOT_ABANDONABLE = "operation.not_abandonable", Conflict, retryable false,
            details ["operationId", "state", "reason"], "the operation cannot be abandoned";
        PAYLOAD_TOO_LARGE = "operation.payload_too_large", Internal, retryable false,
            details ["field", "max"], "a journal payload is above its limit, a bug in the caller";
        JOURNAL_FULL = "operation.journal_full", Internal, retryable false,
            details ["kind", "max"], "an operation recorded more steps or created items than its journal holds";
        INTERRUPTED = "operation.interrupted", Failed, retryable false,
            details [], "the final error of an operation closed after the daemon stopped in the middle of it";
        DAEMON_STOPPING = "operation.daemon_stopping", Unavailable, retryable false,
            details [], "the final error of an operation stopped because the daemon shut down";
        ABANDONED = "operation.abandoned", Failed, retryable false,
            details ["leftovers"], "the final error of an abandoned operation";
        COMPENSATION_FAILED = "operation.compensation_failed", Failed, retryable false,
            details ["item", "detail"], "undoing an item the operation created failed";
    }
}

area! {
    /// Provider package codes (spec 0009, *Kode error di wire*). Each issue
    /// is one `issue` item, with a `reason` field only when the issue code
    /// declares reasons (spec 0021); an issue message never holds a
    /// settings text value or a secret.
    pub mod manifest = "manifest", owner "spec 0009" {
        INVALID = "manifest.invalid", InvalidInput, retryable false,
            details ["package", "issueCount", "omitted"],
            items { kind "issue", fields ["file", "location", "code", "message", "reason"], max 100 },
            "a provider package cannot be read or breaks the manifest schema, and items lists each issue";
    }
}

area! {
    /// Provider engine codes (spec 0009, *Kode error di wire*): settings
    /// and the plans built from a validated package.
    pub mod provider = "provider", owner "spec 0009" {
        INVALID_SETTINGS = "provider.invalid_settings", InvalidInput, retryable false,
            details ["issueCount", "omitted"],
            items { kind "issue", fields ["file", "location", "code", "message", "reason"], max 100 },
            "the settings do not match the provider's settings schema, and items lists each issue";
        UNSUPPORTED_CAPABILITY = "provider.unsupported_capability", Unsupported, retryable false,
            details ["capability"], "the provider lacks the capability the action needs";
        ENDPOINT_NOT_LOOPBACK = "provider.endpoint_not_loopback", InvalidInput, retryable false,
            details ["endpoint"], "an endpoint of the instance is not on a loopback address";
        UNREPRESENTABLE_VALUE = "provider.unrepresentable_value", InvalidInput, retryable false,
            details ["field"], "a value cannot be written in the format of the config file that uses it";
        PLAN_INVALID = "provider.plan_invalid", InvalidInput, retryable false,
            details ["position", "reason"], "an argument or a path of the plan breaks a plan rule";
        RENDER_FAILED = "provider.render_failed", Failed, retryable false,
            details ["configFile", "reason"], "a rendered config file does not parse or is too large";
    }
}

area! {
    /// Provider catalog codes (spec 0017, *Kode error di wire*). None
    /// carries items: catalog issues reach the wire only in
    /// `ProviderDetail.issues`. Every code has a static hint from
    /// [`catalog::hint`](self::catalog::hint).
    pub mod catalog = "catalog", owner "spec 0017" {
        UNAVAILABLE = "catalog.unavailable", Unavailable, retryable false,
            details ["reason"], "the provider catalog is not available";
        PROVIDER_NOT_FOUND = "catalog.provider_not_found", NotFound, retryable false,
            details ["provider", "suggestion"], "no provider with that ID in the catalog";
        VERSION_NOT_FOUND = "catalog.version_not_found", NotFound, retryable false,
            details ["provider", "version"], "the provider has no such version in the catalog";
        NOT_INSTALLABLE = "catalog.not_installable", Unsupported, retryable false,
            details ["provider", "version", "target", "availableTargets", "reason"],
            "the provider version can't be installed on this Mac";
        PIN_FAILED = "catalog.pin_failed", Failed, retryable false,
            details ["provider", "revision", "reason"], "the provider package could not be pinned into the data folder";

        @functions

        /// The static hint of a catalog error, chosen from its code and
        /// the values of its `details`; `None` for a code outside the
        /// area or a `reason` this build does not know. A hint never
        /// holds a value: versions and targets are in the message and the
        /// details.
        #[must_use]
        pub fn hint(code: &str, details: &[crate::error::ErrorDetail]) -> Option<&'static str> {
            let detail = |key: &str| {
                details
                    .iter()
                    .find(|detail| detail.key() == key)
                    .map(crate::error::ErrorDetail::value)
            };
            if code == UNAVAILABLE.code {
                return match detail("reason")? {
                    "not_configured" => Some("run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR"),
                    "missing" => Some("reinstall Stackly, its bundled catalog is missing"),
                    "index_invalid" => Some("reinstall Stackly, its bundled catalog is damaged"),
                    "unsupported_schema" => Some("update Stackly, this catalog needs a newer version"),
                    _ => None,
                };
            }
            if code == PROVIDER_NOT_FOUND.code {
                return Some(if detail("suggestion").is_some() {
                    "use the full provider ID, including its namespace"
                } else {
                    "run stackly provider list to see the catalog"
                });
            }
            if code == VERSION_NOT_FOUND.code {
                return Some("run stackly provider inspect for this provider to see its versions");
            }
            if code == PIN_FAILED.code {
                return Some("run stackly doctor");
            }
            if code == NOT_INSTALLABLE.code {
                return match detail("reason")? {
                    "provider_planned" => Some("this provider is not in this release"),
                    "package_invalid" => Some("reinstall Stackly, this provider package failed validation"),
                    "no_build_for_host" => Some("the catalog has no build of this version for this Mac"),
                    "no_build_for_arch" => Some(
                        "builds exist only for the targets in availableTargets, and Stackly never runs another architecture",
                    ),
                    "artifact_blocked" => Some("this version is blocked in the catalog"),
                    "fixture_only" => Some("test fixtures are never installable"),
                    "requires_newer_os" => Some("this version needs a newer macOS than this Mac runs"),
                    _ => None,
                };
            }
            None
        }
    }
}

area! {
    /// Runtime install codes (spec 0018). The daemon sends them as the
    /// error of a `runtime.*` method and as the `failure.error` of an
    /// `install_runtime` operation; none is retryable on the wire, because
    /// `retryable` here promises the request had no effect. The operation's
    /// own `failure.retryable` says whether trying again can help.
    pub mod runtime = "runtime", owner "spec 0018" {
        SELECTOR_UNRESOLVED = "runtime.selector_unresolved", NotFound, retryable false,
            details ["provider", "selector", "reason", "knownLines"], "the selector matches no version in the catalog";
        SELECTOR_AMBIGUOUS = "runtime.selector_ambiguous", InvalidInput, retryable false,
            details ["selector", "versions"], "the selector matches more than one version";
        SELECTOR_CHANGED = "runtime.selector_changed", Conflict, retryable false,
            details ["expectedSha256", "resolvedSha256"], "the selector now resolves to another artifact than expectedSha256";
        ALREADY_REGISTERED = "runtime.already_registered", Conflict, retryable false,
            details ["runtimeId", "state"], "a runtime for the artifact exists in a state other than installed";
        DOWNLOAD_OFFLINE = "runtime.download_offline", Unavailable, retryable false,
            details ["artifact", "hosts"], "there is no network and no cached download";
        DOWNLOAD_FAILED = "runtime.download_failed", Failed, retryable false,
            details ["artifact", "host", "status", "reason"], "the download failed with an HTTP or connection error";
        DOWNLOAD_UNTRUSTED = "runtime.download_untrusted", Failed, retryable false,
            details ["artifact", "host", "reason"], "the download failed TLS or redirected away from https";
        DOWNLOAD_TOO_LARGE = "runtime.download_too_large", Failed, retryable false,
            details ["artifact", "expected", "received"], "the download length differs from the catalog or the body is longer";
        DIGEST_MISMATCH = "runtime.digest_mismatch", Failed, retryable false,
            details ["artifact", "expected", "actual"], "the SHA-256 of the download is not the catalog's";
        SIZE_MISMATCH = "runtime.size_mismatch", Failed, retryable false,
            details ["artifact", "expected", "actual"], "the size of the download is not the catalog's";
        FORMAT_MISMATCH = "runtime.format_mismatch", Failed, retryable false,
            details ["artifact", "format", "kind"], "the first bytes of the download are not the format the catalog names";
        INSUFFICIENT_SPACE = "runtime.insufficient_space", Failed, retryable false,
            details ["root", "needed", "available"], "there is not enough free space for the download or the extraction";
        IO_FAILED = "runtime.io_failed", Failed, retryable false,
            details ["op", "root"], "a platform port failed with another I/O error";
        ARCHIVE_REJECTED = "runtime.archive_rejected", Failed, retryable false,
            details ["artifact", "reason", "entry"], "the archive breaks an extraction rule";
        ARCHITECTURE_MISMATCH = "runtime.architecture_mismatch", Unsupported, retryable false,
            details ["path", "arches"], "an executable has no arm64 slice, or macOS refused its architecture";
        OS_TOO_OLD = "runtime.os_too_old", Unsupported, retryable false,
            details ["path", "required", "host"], "a binary needs a newer macOS than this Mac runs";
        EXECUTABLE_MISSING = "runtime.executable_missing", Failed, retryable false,
            details ["path", "reason"], "a listed executable is missing or not a regular file";
        DEPENDENCY_OUTSIDE_RUNTIME = "runtime.dependency_outside_runtime", Failed, retryable false,
            details ["path", "library"], "a binary loads a library from outside the runtime";
        DEPENDENCY_MISMATCH = "runtime.dependency_mismatch", Failed, retryable false,
            details ["missing", "unexpected"], "the libraries found differ from the dependencies the catalog lists";
        SIGNATURE_INVALID = "runtime.signature_invalid", Failed, retryable false,
            details ["path", "policy", "detail"], "a file does not meet the signing policy";
        QUARANTINED = "runtime.quarantined", Failed, retryable false,
            details ["path"], "an executable carries the quarantine attribute";
        BINARY_REJECTED_BY_OS = "runtime.binary_rejected_by_os", Failed, retryable false,
            details ["path", "signal"], "macOS killed the binary with SIGKILL when it ran";
        BINARY_CHECK_TIMEOUT = "runtime.binary_check_timeout", Timeout, retryable false,
            details ["path", "seconds"], "the version check did not finish in time";
        BINARY_CHECK_FAILED = "runtime.binary_check_failed", Failed, retryable false,
            details ["path", "exitCode", "signal", "errno"], "the version check exited non zero, died of a signal other than SIGKILL, or could not start";
        VERSION_MISMATCH = "runtime.version_mismatch", Failed, retryable false,
            details ["expected", "observed"], "the version the binary reports is not the catalog's";
        INSTALL_CONFLICT = "runtime.install_conflict", Conflict, retryable false,
            details ["path"], "the target folder belongs to something else";

        @functions

        /// The static hint of a runtime error, chosen from its code; `None`
        /// for a code outside the area. A hint never holds a value:
        /// versions, paths, hosts, and sizes are in the message and the
        /// details.
        #[must_use]
        pub fn hint(code: &str, _details: &[crate::error::ErrorDetail]) -> Option<&'static str> {
            Some(match code {
                "runtime.selector_unresolved" => {
                    "Run stackly provider inspect for this provider to see its versions."
                }
                "runtime.selector_ambiguous" => {
                    "Name an exact version or line so only one version matches."
                }
                "runtime.selector_changed" => {
                    "The catalog changed since you chose. Review the new artifact and try again."
                }
                "runtime.already_registered" => {
                    "A runtime for this artifact exists but is not installed. Inspect it before you try again."
                }
                "runtime.download_offline" => {
                    "Connect to the internet and try again. Stackly keeps any partial download."
                }
                "runtime.download_failed" => {
                    "Check your connection and try again. Stackly keeps any partial download."
                }
                "runtime.download_untrusted" => {
                    "Stackly only downloads over verified HTTPS. Trying again will not help; report this catalog entry."
                }
                "runtime.download_too_large" => {
                    "The download is larger than the catalog says. Try again later, or report this catalog entry."
                }
                "runtime.digest_mismatch" | "runtime.size_mismatch" => {
                    "The download does not match the catalog. Try again; if it repeats, report this catalog entry."
                }
                "runtime.format_mismatch" => {
                    "The download is not the kind of file the catalog names. Report this catalog entry."
                }
                "runtime.insufficient_space" => {
                    "Free space on the disk that holds Stackly data, then try again."
                }
                "runtime.io_failed" => {
                    "Check that Stackly can write to its data folder, then try again."
                }
                "runtime.archive_rejected" => {
                    "The archive breaks Stackly's safety rules, so nothing was extracted. Report this catalog entry."
                }
                "runtime.architecture_mismatch" => {
                    "This artifact is not built for Apple Silicon, and Stackly does not run it under Rosetta."
                }
                "runtime.os_too_old" => {
                    "This build needs a newer macOS than this Mac runs. Update macOS or choose an older version."
                }
                "runtime.executable_missing" => {
                    "The artifact lacks a program the catalog lists. Report this catalog entry."
                }
                "runtime.dependency_outside_runtime" => {
                    "A binary loads a library from outside the runtime, so Stackly refused it. Report this catalog entry."
                }
                "runtime.dependency_mismatch" => {
                    "The libraries in the artifact differ from the catalog's list. Report this catalog entry."
                }
                "runtime.signature_invalid" => {
                    "The files are not signed the way the catalog's policy needs. Try again; if it repeats, report this catalog entry."
                }
                "runtime.quarantined" | "runtime.binary_rejected_by_os" => {
                    "Stackly never removes quarantine. Remove this runtime and install it again from Stackly."
                }
                "runtime.binary_check_timeout" => {
                    "macOS may be showing a security prompt. Check for a dialog, then try again."
                }
                "runtime.binary_check_failed" => {
                    "The runtime did not run for its version check. Read the details, then try again."
                }
                "runtime.version_mismatch" => {
                    "The binary reports another version than the catalog. Report this catalog entry."
                }
                "runtime.install_conflict" => {
                    "A folder Stackly did not create is in the way. Stackly never deletes it; move it away and try again."
                }
                _ => return None,
            })
        }
    }
}

area! {
    /// Secret store and redaction codes (spec 0011, *Kode error di wire*).
    /// None is retryable: even a code that is safe to repeat needs the
    /// person to act first, such as filling a value or unlocking the
    /// keychain. The format of a reference and the basic rules of a value
    /// (UTF-8, length, NUL) are `request.invalid_params`; these codes are
    /// for rules that need state. `secret.invalid_value` may carry
    /// `fields` and never `items`; every other list is `items`.
    pub mod secret = "secret", owner "spec 0011" {
        NEEDS_SECRET = "secret.needs_secret", Unavailable, retryable false,
            details ["count", "omitted"],
            items { kind "reference", fields ["reference", "reason", "generateAllowed"], max 100 },
            "a resource needs references that cannot be read, or a create or restore names a source reference that cannot be read";
        NOT_FOUND = "secret.not_found", NotFound, retryable false,
            details ["reference"], "the reference is not registered, or a replace names an item that does not exist";
        CONFLICT = "secret.conflict", Conflict, retryable false,
            details ["reference", "reason", "omitted"],
            items { kind "use", fields ["resourceKind", "resourceId", "field"], max 100 },
            "a create names an item that exists, or a delete names a reference that is still used";
        CONFIRMATION_REQUIRED = "secret.confirmation_required", InvalidInput, retryable false,
            details ["reference", "omitted"],
            items { kind "effect", fields ["resourceKind", "resourceId", "field", "effect"], max 100 },
            "a replace changes only the stored copy and needs confirmStoredOnly";
        INVALID_REFERENCE = "secret.invalid_reference", InvalidInput, retryable false,
            details ["reference", "reason"], "the reference names the reserved first segment and is not registered";
        INVALID_VALUE = "secret.invalid_value", InvalidInput, retryable false,
            details ["field", "minLength", "maxLength", "reason", "omitted"],
            "the value breaks the length or character rules of a secret field that uses the reference";
        DELIVERY_REJECTED = "secret.delivery_rejected", Failed, retryable false,
            details ["reason"], "an argument would carry a secret value to a provider process";
        STORE_LOCKED = "secret.store_locked", Unavailable, retryable false,
            details [], "the keychain is locked";
        STORE_ACCESS_DENIED = "secret.store_access_denied", Unauthorized, retryable false,
            details ["byUser"], "the person refused the dialog, or the access list does not trust the daemon";
        STORE_BUSY = "secret.store_busy", Unavailable, retryable false,
            details [], "the store job did not start in time";
        STORE_OUTCOME_UNKNOWN = "secret.store_outcome_unknown", Timeout, retryable false,
            details [], "a write or delete passed its wait limit, or putting the old value back failed";
        STORE_UNSUPPORTED = "secret.store_unsupported", Unsupported, retryable false,
            details [], "this OS has no secret store";
        STORE_BACKEND = "secret.store_backend", Failed, retryable false,
            details ["code"], "the store failed with another OSStatus";
    }
}

area! {
    /// Supervisor codes (spec 0010, *Kode error*): why an instance cannot
    /// start, stop, or run. Every code is `retryable false` except the two
    /// that say a retry may help. `supervisor.endpoint_in_use`,
    /// `supervisor.endpoint_conflict`, and `supervisor.endpoint_unavailable`
    /// may carry `fields` and never `items`; every other list is `items`.
    pub mod supervisor = "supervisor", owner "spec 0010" {
        NOT_INITIALIZED = "supervisor.not_initialized", Conflict, retryable false,
            details ["lifecycle"], "a start before the data folder was initialized";
        INSTANCE_INITIALIZING = "supervisor.instance_initializing", Conflict, retryable false,
            details [], "a stop while the data folder is being initialized";
        INSTANCE_STOPPING = "supervisor.instance_stopping", Conflict, retryable false,
            details [], "a start while the instance is stopping";
        INSTANCE_RETAINED = "supervisor.instance_retained", Conflict, retryable false,
            details [], "an action on a retained instance";
        IDENTITY_UNPROVEN = "supervisor.identity_unproven", Conflict, retryable false,
            details ["pid", "omitted"],
            items { kind "check", fields ["name", "result"], max 7 },
            "a start or stop on an instance whose process cannot be proven to be its own";
        DATA_IN_USE = "supervisor.data_in_use", Conflict, retryable false,
            details ["lockPath", "pid"], "the instance lock is held, or the last observed process still runs";
        ENDPOINT_IN_USE = "supervisor.endpoint_in_use", Conflict, retryable false,
            details ["endpoint", "address", "port", "omitted"], "another process holds the port of an endpoint";
        ENDPOINT_UNAVAILABLE = "supervisor.endpoint_unavailable", Unavailable, retryable true,
            details ["endpoint", "address", "port", "reason", "omitted"],
            "the os refused the trial bind of a fixed port, or the address is not available";
        ENDPOINT_CONFLICT = "supervisor.endpoint_conflict", Conflict, retryable false,
            details ["endpoint", "port", "otherInstance", "omitted"], "the effective port belongs to another instance";
        AUTO_PORT_UNAVAILABLE = "supervisor.auto_port_unavailable", Unavailable, retryable true,
            details ["endpoint"], "no free candidate port for an automatic endpoint";
        RUNTIME_UNAVAILABLE = "supervisor.runtime_unavailable", Conflict, retryable false,
            details ["runtimeId", "state"], "the runtime or one it depends on is not installed";
        PACKAGE_UNAVAILABLE = "supervisor.package_unavailable", Failed, retryable false,
            details ["provider", "reason"], "the pinned package is missing, invalid, or has another digest";
        CONFIG_INVALID = "supervisor.config_invalid", Failed, retryable false,
            details ["field", "reason"], "the settings or endpoints no longer fit the plan of the package";
        EXECUTABLE_INVALID = "supervisor.executable_invalid", Failed, retryable false,
            details ["reason"], "the executable is not a canonical regular file inside its runtime";
        DATA_UNAVAILABLE = "supervisor.data_unavailable", Failed, retryable false,
            details ["path", "reason"], "the instance folder is missing, not the user's, or not private";
        DATA_INCOMPATIBLE = "supervisor.data_incompatible", Conflict, retryable false,
            details ["outcome", "data", "runtime"], "the data and the runtime are not compatible";
        READINESS_TIMEOUT = "supervisor.readiness_timeout", Timeout, retryable false,
            details ["deadlineSeconds"], "the readiness deadline passed and the process keeps running";
        RETRY_BUDGET_EXHAUSTED = "supervisor.retry_budget_exhausted", Failed, retryable false,
            details ["attempts", "lastExitCode", "lastSignal"], "the restart budget ran out during a start";
        STOP_TIMEOUT = "supervisor.stop_timeout", Timeout, retryable false,
            details ["timeoutSeconds"], "the stop budget ran out";
        STOP_REQUIRES_OVERRIDE = "supervisor.stop_requires_override", Conflict, retryable false,
            details ["omitted"],
            items { kind "environment", fields ["environmentId"], max 100 },
            "an active environment lease needs an override to stop the instance";
        STOP_COMMAND_FAILED = "supervisor.stop_command_failed", Failed, retryable false,
            details ["exitCode"], "the stop command exited with a code it does not accept";
        RELOAD_FAILED = "supervisor.reload_failed", Failed, retryable false,
            details ["reason"], "the reload action or its verification failed";
        CAPABILITY_UNSUPPORTED = "supervisor.capability_unsupported", Unsupported, retryable false,
            details ["capability"], "the provider lacks the capability for an action on an existing instance";
        INIT_FAILED = "supervisor.init_failed", Failed, retryable false,
            details ["reason", "step", "exitCode"], "the init plan ended failed";

        @functions

        /// The static hint of a supervisor error, chosen from its code and
        /// the values of its `details` (spec 0025, *Hint migrasi*): only
        /// `supervisor.data_incompatible` has one, by `outcome`. The text
        /// holds no value; lines and paths are in the message and the
        /// details. `None` for `unknown`, an `outcome` it does not know, and
        /// any other code.
        #[must_use]
        pub fn hint(code: &str, details: &[crate::error::ErrorDetail]) -> Option<&'static str> {
            if code != DATA_INCOMPATIBLE.code {
                return None;
            }
            let outcome = details
                .iter()
                .find(|detail| detail.key() == "outcome")
                .map(crate::error::ErrorDetail::value)?;
            match outcome {
                "incompatible" => Some(
                    "This data belongs to another major version, so this runtime never runs it. Moving it needs a migration: create an instance on this runtime and move the data with a dump and restore.",
                ),
                "marker_missing" | "marker_mismatch" | "marker_unreadable" => Some(
                    "The version marker in the data folder is missing, unreadable, or not what Stackly recorded, so Stackly can’t tell which runtime may run this data and won’t run it until the marker matches.",
                ),
                _ => None,
            }
        }
    }
}

area! {
    /// Process codes (spec 0010, *Kode error*): what went wrong with one
    /// operating system process.
    pub mod process = "process", owner "spec 0010" {
        SPAWN_FAILED = "process.spawn_failed", Failed, retryable false,
            details ["kind"], "the os could not spawn the process";
        IDENTITY_CAPTURE_FAILED = "process.identity_capture_failed", Failed, retryable false,
            details [], "the identity of the suspended child could not be captured or recorded";
        SIGNAL_REFUSED = "process.signal_refused", Conflict, retryable false,
            details ["pid"], "the identity changed at the check right before a signal";
        UNSUPPORTED = "process.unsupported", Unsupported, retryable false,
            details ["os"], "this platform has no process supervision";
    }
}

area! {
    /// Instance codes (spec 0010, *Kode error*): the synchronous refusals
    /// of `instance.create`. Spec 0020 adds the codes of its own evaluator
    /// to this area. `instance.settings_invalid` and
    /// `instance.endpoint_invalid` may carry `fields` and never `items`.
    pub mod instance = "instance", owner "spec 0010" {
        NAME_TAKEN = "instance.name_taken", Conflict, retryable false,
            details ["existing"], "another instance has the name";
        PROVIDER_NOT_FOUND = "instance.provider_not_found", NotFound, retryable false,
            details ["provider", "revision"], "no pinned package or provider revision with that name";
        PROVIDER_AMBIGUOUS = "instance.provider_ambiguous", Conflict, retryable false,
            details ["provider", "revision"], "more than one package digest matches the provider and revision";
        RUNTIME_NOT_FOUND = "instance.runtime_not_found", NotFound, retryable false,
            details ["runtimeId"], "no runtime with that id";
        RUNTIME_MISMATCH = "instance.runtime_mismatch", Conflict, retryable false,
            details ["runtimeSoftware", "providerSoftware"], "the runtime software differs from the provider software";
        SETTINGS_INVALID = "instance.settings_invalid", InvalidInput, retryable false,
            details ["omitted"], "the settings break the schema of the package; fields lists each issue";
        ENDPOINT_INVALID = "instance.endpoint_invalid", InvalidInput, retryable false,
            details ["endpoint", "omitted"], "an endpoint is not in the manifest, has a port outside 1 to 65535, asks for a port on a unix endpoint, or is Auto while the provider’s initialize needs its port";
        NAME_INVALID = "instance.name_invalid", InvalidInput, retryable false,
            details ["omitted"], "the instance name or display name is not valid; fields lists each issue";
        PROVIDER_UNAVAILABLE = "instance.provider_unavailable", Unsupported, retryable false,
            details ["provider", "state"], "the catalog entry of the provider is planned or invalid";
        CAPABILITY_UNSUPPORTED = "instance.capability_unsupported", Unsupported, retryable false,
            details ["capability", "provider", "existing"], "the provider lacks multi_instance and an instance of it already exists";
        RUNTIME_INCOMPATIBLE = "instance.runtime_incompatible", Conflict, retryable false,
            details ["runtimeId", "executable"], "the runtime lacks an executable the package needs";
        STORAGE_UNSUPPORTED = "instance.storage_unsupported", Unsupported, retryable false,
            details ["fsType", "root"], "the data root is not on a local apfs or hfs volume";
        INSUFFICIENT_SPACE = "instance.insufficient_space", Unavailable, retryable false,
            details ["needed", "available"], "the free space is below the minimum to create an instance";
        DATA_NOT_EMPTY = "instance.data_not_empty", Conflict, retryable false,
            details ["path"], "the new instance folder already exists or its data folder is not empty";
        DATA_OVERLAP = "instance.data_overlap", Conflict, retryable false,
            details ["path", "otherInstance"], "the new data folder overlaps the data folder of another instance";
        SHORT_KEY_TAKEN = "instance.short_key_taken", Conflict, retryable false,
            details ["shortKey"], "the short key of the new instance ID is taken";
        DATA_USAGE_UNAVAILABLE = "instance.data_usage_unavailable", Unavailable, retryable false,
            details ["reason"], "the data folder size can't be measured";

        @functions

        /// The static hint of an instance error, chosen from its code and
        /// the values of its `details`; `None` for a code without a hint or
        /// a `state` this build does not know. A hint never holds a value.
        #[must_use]
        pub fn hint(code: &str, details: &[crate::error::ErrorDetail]) -> Option<&'static str> {
            Some(match code {
                "instance.endpoint_invalid" => {
                    "check the endpoint name and port; some providers need a fixed port for an endpoint when the instance is created"
                }
                "instance.name_invalid" => {
                    "use lowercase letters, digits, and single hyphens, starting with a letter"
                }
                "instance.provider_unavailable" => {
                    return match details
                        .iter()
                        .find(|detail| detail.key() == "state")
                        .map(crate::error::ErrorDetail::value)?
                    {
                        "planned" => Some("this provider is not in this release"),
                        "invalid" => {
                            Some("reinstall Stackly, this provider package failed validation")
                        }
                        _ => None,
                    };
                }
                "instance.capability_unsupported" => "remove the existing instance first",
                "instance.runtime_incompatible" => {
                    "reinstall the runtime with stackly runtime install"
                }
                "instance.storage_unsupported" => {
                    "keep the Stackly data folder on a local APFS volume"
                }
                "instance.insufficient_space" => "free up disk space, then try again",
                "instance.data_not_empty" => {
                    "run stackly doctor; Stackly never initializes a folder that already holds data"
                }
                "instance.data_overlap" => "run stackly doctor",
                "instance.short_key_taken" => "try again, a new ID is generated",
                "instance.data_usage_unavailable" => {
                    return match details
                        .iter()
                        .find(|detail| detail.key() == "reason")
                        .map(crate::error::ErrorDetail::value)?
                    {
                        "missing" => Some("run stackly doctor; the data folder is missing"),
                        "not_private" => Some(
                            "run stackly doctor; the data folder isn't a private folder you own, or part of it can't be read",
                        ),
                        "io" => Some("try again"),
                        _ => None,
                    };
                }
                _ => return None,
            })
        }
    }
}

area! {
    /// Storage codes (spec 0022). The one code is retryable because
    /// reading the volume changes nothing.
    pub mod storage = "storage", owner "spec 0022" {
        CAPACITY_UNAVAILABLE = "storage.capacity_unavailable", Unavailable, retryable true,
            details ["reason", "kind"],
            "the space left on the volume that holds the data folder cannot be read";

        @functions

        /// The static hint of a storage error, chosen from its code and the
        /// values of its `details`; `None` for a code outside the area or a
        /// `reason` without a hint.
        #[must_use]
        pub fn hint(code: &str, details: &[crate::error::ErrorDetail]) -> Option<&'static str> {
            if code != CAPACITY_UNAVAILABLE.code {
                return None;
            }
            let reason = details
                .iter()
                .find(|detail| detail.key() == "reason")
                .map(crate::error::ErrorDetail::value)?;
            match reason {
                "timeout" => Some(
                    "The disk that holds Stackly data didn’t answer. Check that it is still connected.",
                ),
                _ => None,
            }
        }
    }
}

area! {
    /// Codes for bugs and broken invariants.
    pub mod internal = "internal", owner "spec 0006" {
        UNEXPECTED = "internal.unexpected", Internal, retryable false,
            details [], "a handler panicked or an invariant broke";
    }
}

area! {
    /// Codes the CLI makes itself (spec 0008, *Kode error area cli*). They
    /// only ever appear in `stackly` output, never on the wire, so none is
    /// retryable and the area stays out of the contract digest. A code about
    /// an operation also carries `operationId` and the operation resource.
    pub mod cli = "cli", owner "spec 0008" {
        USAGE = "cli.usage", InvalidInput, retryable false,
            details ["kind"], "the arguments do not parse, written as an envelope only with --json";
        INVALID_ENVIRONMENT = "cli.invalid_environment", InvalidInput, retryable false,
            details ["variable", "reason"], "an environment variable the cli reads holds a value it refuses";
        PLATFORM_UNAVAILABLE = "cli.platform_unavailable", Unavailable, retryable false,
            details ["reason"], "the platform roots or the socket path cannot be worked out";
        INPUT_REQUIRED = "cli.input_required", InvalidInput, retryable false,
            details ["input"], "a prompt is needed while the cli runs noninteractive";
        CONFIRMATION_REQUIRED = "cli.confirmation_required", InvalidInput, retryable false,
            details ["flag"], "a confirmation is needed while the cli runs noninteractive without the approval flag";
        CONFIRMATION_DECLINED = "cli.confirmation_declined", Cancelled, retryable false,
            details [], "the person at the terminal did not answer y or yes";
        WAIT_TIMEOUT = "cli.wait_timeout", Timeout, retryable false,
            details ["state", "timeoutSeconds"], "the wait timeout ran out before the operation reached a final state";
        WAIT_DISCONNECTED = "cli.wait_disconnected", Unavailable, retryable false,
            details ["state"], "the connection to the daemon did not come back within 30 seconds while waiting";
        INTERRUPTED = "cli.interrupted", Cancelled, retryable false,
            details ["state"], "ctrl+c stopped the wait while the operation keeps running, exit 130";
        CANCEL_NOT_APPLIED = "cli.cancel_not_applied", Conflict, retryable false,
            details ["state"], "the operation ended in another state than cancelled after a cancel";
        OPERATION_FAILED = "cli.operation_failed", Failed, retryable false,
            details [], "fallback for an operation that failed without an error in its summary";
        OPERATION_CANCELLED = "cli.operation_cancelled", Cancelled, retryable false,
            details [], "fallback for an operation that was cancelled, which carries no error";
        OPERATION_NEEDS_RECOVERY = "cli.operation_needs_recovery", Failed, retryable false,
            details [], "fallback for an operation that needs recovery without an error in its summary";
        INSTANCE_NOT_FOUND = "cli.instance_not_found", NotFound, retryable false,
            details ["name"], "no instance has the name the command was given";
        RUNTIME_NOT_FOUND = "cli.runtime_not_found", NotFound, retryable false,
            details ["selector"], "no installed runtime matches the software@version the command was given";
        RUNTIME_AMBIGUOUS = "cli.runtime_ambiguous", Conflict, retryable false,
            details ["selector", "candidates"], "more than one installed runtime matches the software@version the command was given";
        PANIC = "cli.panic", Internal, retryable false,
            details [], "the cli panicked, and the panic message is never shown";
        AGENT_ROOT_OVERRIDE = "cli.agent_root_override", InvalidInput, retryable false,
            details ["variable"], "an agent command was asked while STACKLY_ROOT is set, and the agent only serves the default root (spec 0015)";
        AGENT_NOT_BUNDLED = "cli.agent_not_bundled", Unsupported, retryable false,
            details ["executable"], "the cli does not run from inside the Stackly app bundle, so macOS cannot be asked about the agent (spec 0015)";
        AGENT_HELPER_MISSING = "cli.agent_helper_missing", Unavailable, retryable false,
            details ["bundlePath"], "the app bundle lacks the LaunchAgent property list or stacklyd (spec 0015)";
        AGENT_NOT_REGISTERED = "cli.agent_not_registered", Unavailable, retryable false,
            details ["label"], "the background agent is not registered, so there is nothing to start (spec 0015)";
        AGENT_REQUIRES_APPROVAL = "cli.agent_requires_approval", Unauthorized, retryable false,
            details ["label"], "the background agent waits for the user's approval in Login Items (spec 0015)";
        AGENT_DISABLED = "cli.agent_disabled", Unauthorized, retryable false,
            details ["label"], "the background agent was switched off in Login Items after it ran before (spec 0015)";
        AGENT_NOT_FOUND = "cli.agent_not_found", Unavailable, retryable false,
            details ["label", "bundlePath"], "macOS cannot find the service for this copy of the app right after the registration (spec 0015)";
        AGENT_INVALID_SIGNATURE = "cli.agent_invalid_signature", Unauthorized, retryable false,
            details ["label", "osCode"], "macOS rejected the code signature of the app bundle (spec 0015)";
        AGENT_OS_ERROR = "cli.agent_os_error", Failed, retryable false,
            details ["op", "osDomain", "osCode"], "a register, unregister, or relaunch call failed in macOS (spec 0015)";
        AGENT_STATUS_UNAVAILABLE = "cli.agent_status_unavailable", Unavailable, retryable false,
            details ["op", "osCode"], "the agent status cannot be read or holds a value this build does not know (spec 0015)";
        AGENT_CALL_TIMEOUT = "cli.agent_call_timeout", Timeout, retryable false,
            details ["op", "seconds"], "a macOS call about the agent took longer than 10 seconds (spec 0015)";
        DAEMON_WAIT_TIMEOUT = "cli.daemon_wait_timeout", Timeout, retryable false,
            details ["action", "seconds"], "the daemon did not appear or disappear within the limit of the command (spec 0015)";
        CONNECTION_NOT_FOUND = "cli.connection_not_found", NotFound, retryable false,
            details ["format", "available"], "no connection string has the name the command was given, or the provider describes none (spec 0021)";
        CONNECTION_UNAVAILABLE = "cli.connection_unavailable", Conflict, retryable false,
            details ["format", "reason"], "the selected connection string cannot be built now, or the pinned package cannot be read (spec 0021)";
    }
}

/// Every registered area. A spec that needs a new area adds it here.
pub const AREAS: &[Area] = &[
    protocol::AREA,
    ipc::AREA,
    request::AREA,
    events::AREA,
    resource::AREA,
    daemon::AREA,
    registry::AREA,
    operation::AREA,
    manifest::AREA,
    provider::AREA,
    catalog::AREA,
    runtime::AREA,
    secret::AREA,
    supervisor::AREA,
    process::AREA,
    instance::AREA,
    storage::AREA,
    internal::AREA,
    cli::AREA,
];

/// Every code of every area, area by area.
pub fn catalog() -> impl Iterator<Item = &'static ErrorSpec> {
    AREAS.iter().flat_map(|area| area.codes.iter())
}

/// The catalog entry for `code`, or `None` when this build does not know it.
#[must_use]
pub fn lookup(code: &str) -> Option<&'static ErrorSpec> {
    catalog().find(|spec| spec.code == code)
}

/// Why `ipc.endpoint_untrusted` refused an endpoint, the fixed values of its
/// `reason` detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UntrustedReason {
    /// The path is not a socket.
    NotASocket,
    /// The path is a symbolic link.
    Symlink,
    /// Another user owns the socket.
    WrongOwner,
    /// The socket grants access to its group or to others.
    GroupOrOtherAccess,
    /// Another user owns the folder holding the socket.
    ParentWrongOwner,
    /// The folder holding the socket grants access to its group or others.
    ParentGroupOrOtherAccess,
    /// The server peer runs as another user.
    PeerUidMismatch,
    /// The server peer's credentials cannot be read.
    PeerCredentialsUnavailable,
}

impl UntrustedReason {
    /// Every reason.
    pub const ALL: [Self; 8] = [
        Self::NotASocket,
        Self::Symlink,
        Self::WrongOwner,
        Self::GroupOrOtherAccess,
        Self::ParentWrongOwner,
        Self::ParentGroupOrOtherAccess,
        Self::PeerUidMismatch,
        Self::PeerCredentialsUnavailable,
    ];

    /// The detail value, such as `wrong_owner`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotASocket => "not_a_socket",
            Self::Symlink => "symlink",
            Self::WrongOwner => "wrong_owner",
            Self::GroupOrOtherAccess => "group_or_other_access",
            Self::ParentWrongOwner => "parent_wrong_owner",
            Self::ParentGroupOrOtherAccess => "parent_group_or_other_access",
            Self::PeerUidMismatch => "peer_uid_mismatch",
            Self::PeerCredentialsUnavailable => "peer_credentials_unavailable",
        }
    }
}

/// Why `operation.resume` refused, the fixed values of the `reason` detail
/// of `operation.not_resumable` (spec 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotResumableReason {
    /// The operation is not in state `needs_recovery`.
    WrongState,
    /// The operation's kind never resumes.
    KindNotResumable,
    /// The step that failed or was interrupted cannot run again.
    StepNotRepeatable,
    /// No work factory is registered for the operation's kind.
    NoFactory,
    /// Undoing the operation already started or failed.
    CompensationFailed,
    /// The engine is undoing the operation on its own right now.
    AutoCompensating,
}

impl NotResumableReason {
    /// Every reason.
    pub const ALL: [Self; 6] = [
        Self::WrongState,
        Self::KindNotResumable,
        Self::StepNotRepeatable,
        Self::NoFactory,
        Self::CompensationFailed,
        Self::AutoCompensating,
    ];

    /// The detail value, such as `wrong_state`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrongState => "wrong_state",
            Self::KindNotResumable => "kind_not_resumable",
            Self::StepNotRepeatable => "step_not_repeatable",
            Self::NoFactory => "no_factory",
            Self::CompensationFailed => "compensation_failed",
            Self::AutoCompensating => "auto_compensating",
        }
    }
}

/// Why `operation.abandon` refused, the fixed values of the `reason`
/// detail of `operation.not_abandonable` (spec 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotAbandonableReason {
    /// The operation is not in state `needs_recovery`.
    WrongState,
    /// The engine is undoing the operation on its own right now.
    AutoCompensating,
}

impl NotAbandonableReason {
    /// Every reason.
    pub const ALL: [Self; 2] = [Self::WrongState, Self::AutoCompensating];

    /// The detail value, such as `auto_compensating`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrongState => "wrong_state",
            Self::AutoCompensating => "auto_compensating",
        }
    }
}

/// Why a plan broke a plan rule, the fixed values of the `reason` detail
/// of `provider.plan_invalid` (spec 0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlanInvalidReason {
    /// An argument is longer than 4096 bytes after rendering.
    ArgumentTooLong,
    /// A path from the context is not UTF-8.
    PathNotUtf8,
    /// A path from the context is not absolute, or holds `.` or `..`.
    PathEscapesScope,
}

impl PlanInvalidReason {
    /// Every reason.
    pub const ALL: [Self; 3] = [
        Self::ArgumentTooLong,
        Self::PathNotUtf8,
        Self::PathEscapesScope,
    ];

    /// The detail value, such as `argument_too_long`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArgumentTooLong => "argument_too_long",
            Self::PathNotUtf8 => "path_not_utf8",
            Self::PathEscapesScope => "path_escapes_scope",
        }
    }
}

/// Why rendering a config file failed, the fixed values of the `reason`
/// detail of `provider.render_failed` (spec 0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RenderFailedReason {
    /// A rendered `yaml` or `json` file does not parse.
    RenderedOutputInvalid,
    /// A rendered file is larger than 1 MiB.
    OutputTooLarge,
}

impl RenderFailedReason {
    /// Every reason.
    pub const ALL: [Self; 2] = [Self::RenderedOutputInvalid, Self::OutputTooLarge];

    /// The detail value, such as `output_too_large`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RenderedOutputInvalid => "rendered_output_invalid",
            Self::OutputTooLarge => "output_too_large",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// `^[a-z][a-z0-9_]*$`.
    fn is_snake(text: &str) -> bool {
        let mut bytes = text.bytes();
        bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    }

    /// `^[a-z][a-zA-Z0-9]*$`.
    fn is_camel(text: &str) -> bool {
        let mut bytes = text.bytes();
        bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            && bytes.all(|b| b.is_ascii_alphanumeric())
    }

    #[test]
    fn every_code_matches_the_area_dot_reason_pattern() {
        for spec in catalog() {
            let (area, reason) = spec.code.split_once('.').unwrap();
            assert!(
                is_snake(area) && is_snake(reason),
                "`{}` does not match ^[a-z][a-z0-9_]*\\.[a-z][a-z0-9_]*$",
                spec.code
            );
        }
    }

    #[test]
    fn every_code_sits_in_the_area_its_prefix_names() {
        for area in AREAS {
            assert!(
                is_snake(area.name),
                "area `{}` is not snake case",
                area.name
            );
            assert!(!area.codes.is_empty(), "area `{}` has no codes", area.name);
            for spec in area.codes {
                assert_eq!(
                    spec.code.split_once('.').map(|(prefix, _)| prefix),
                    Some(area.name),
                    "`{}` is listed under area `{}`",
                    spec.code,
                    area.name
                );
            }
        }
    }

    #[test]
    fn no_code_or_area_appears_twice() {
        let mut codes = HashSet::new();
        for spec in catalog() {
            assert!(codes.insert(spec.code), "`{}` appears twice", spec.code);
        }
        let mut areas = HashSet::new();
        for area in AREAS {
            assert!(
                areas.insert(area.name),
                "area `{}` appears twice",
                area.name
            );
        }
    }

    #[test]
    fn details_keys_are_camel_case_and_unique_per_code() {
        for spec in catalog() {
            let mut keys = HashSet::new();
            for key in spec.details {
                assert!(
                    is_camel(key),
                    "`{}` detail key `{key}` is not camelCase",
                    spec.code
                );
                assert!(keys.insert(key), "`{}` lists `{key}` twice", spec.code);
            }
        }
    }

    #[test]
    fn items_name_a_snake_case_kind_camel_case_fields_a_limit_and_omitted() {
        for spec in catalog() {
            let Some(items) = spec.items else { continue };
            assert!(
                is_snake(items.kind),
                "`{}` item kind is not snake case",
                spec.code
            );
            assert!(items.max >= 1, "`{}` allows no item", spec.code);
            let mut keys = HashSet::new();
            for field in items.fields {
                assert!(
                    is_camel(field),
                    "`{}` item field `{field}` is not camelCase",
                    spec.code
                );
                assert!(keys.insert(field), "`{}` lists `{field}` twice", spec.code);
            }
            assert!(
                spec.details.contains(&crate::error::OMITTED_DETAIL),
                "`{}` has items but no `omitted` detail",
                spec.code
            );
        }
    }

    #[test]
    fn the_area_macro_records_items_only_where_a_code_names_them() {
        area! {
            /// A test area.
            pub mod sample = "sample", owner "test" {
                PLAIN = "sample.plain", Failed, retryable false,
                    details [], "a code without items";
                LISTED = "sample.listed", Failed, retryable false,
                    details ["omitted"],
                    items { kind "issue", fields ["path", "code"], max 50 },
                    "a code with items";
            }
        }
        assert_eq!(sample::PLAIN.items, None);
        assert_eq!(
            sample::LISTED.items,
            Some(ItemSpec {
                kind: "issue",
                fields: &["path", "code"],
                max: 50
            })
        );
        assert_eq!(sample::AREA.codes.len(), 2);
    }

    #[test]
    fn summaries_are_lowercase_sentences_without_a_trailing_period() {
        for spec in catalog() {
            assert!(
                spec.summary.starts_with(|c: char| c.is_ascii_lowercase()),
                "`{}` summary does not start lowercase",
                spec.code
            );
            assert!(
                !spec.summary.ends_with('.'),
                "`{}` summary ends with a period",
                spec.code
            );
            assert!(
                spec.summary.len() <= 200,
                "`{}` summary is too long",
                spec.code
            );
        }
    }

    #[test]
    fn retryable_codes_are_only_ones_that_promise_no_effect() {
        let retryable: Vec<_> = catalog()
            .filter(|spec| spec.retryable)
            .map(|spec| spec.code)
            .collect();
        assert_eq!(
            retryable,
            [
                "ipc.endpoint_missing",
                "ipc.connection_refused",
                "ipc.io_error",
                "ipc.too_many_connections",
                "ipc.not_connected",
                "request.too_many_in_flight",
                "resource.busy",
                "daemon.starting",
                "daemon.shutting_down",
                "operation.queue_full",
                "supervisor.endpoint_unavailable",
                "supervisor.auto_port_unavailable",
                "storage.capacity_unavailable",
            ]
        );
    }

    #[test]
    fn lookup_finds_every_code_and_nothing_else() {
        for spec in catalog() {
            assert_eq!(lookup(spec.code), Some(spec));
        }
        assert_eq!(lookup("ipc.nope"), None);
        assert_eq!(lookup(""), None);
    }

    #[test]
    fn every_area_is_registered_with_its_owning_spec() {
        let owners: Vec<_> = AREAS.iter().map(|area| (area.name, area.owner)).collect();
        assert_eq!(
            owners,
            [
                ("protocol", "spec 0006"),
                ("ipc", "spec 0006"),
                ("request", "spec 0006"),
                ("events", "spec 0006"),
                ("resource", "spec 0006"),
                ("daemon", "spec 0006"),
                ("registry", "spec 0005"),
                ("operation", "spec 0007"),
                ("manifest", "spec 0009"),
                ("provider", "spec 0009"),
                ("catalog", "spec 0017"),
                ("runtime", "spec 0018"),
                ("secret", "spec 0011"),
                ("supervisor", "spec 0010"),
                ("process", "spec 0010"),
                ("instance", "spec 0010"),
                ("storage", "spec 0022"),
                ("internal", "spec 0006"),
                ("cli", "spec 0008"),
            ]
        );
    }

    #[test]
    fn cli_codes_match_the_spec_0008_table() {
        let table: Vec<_> = cli::ALL
            .iter()
            .map(|spec| (spec.code, spec.category, spec.details))
            .collect();
        let expected: &[(&str, ErrorCategory, &[&str])] = &[
            ("cli.usage", ErrorCategory::InvalidInput, &["kind"]),
            (
                "cli.invalid_environment",
                ErrorCategory::InvalidInput,
                &["variable", "reason"],
            ),
            (
                "cli.platform_unavailable",
                ErrorCategory::Unavailable,
                &["reason"],
            ),
            (
                "cli.input_required",
                ErrorCategory::InvalidInput,
                &["input"],
            ),
            (
                "cli.confirmation_required",
                ErrorCategory::InvalidInput,
                &["flag"],
            ),
            ("cli.confirmation_declined", ErrorCategory::Cancelled, &[]),
            (
                "cli.wait_timeout",
                ErrorCategory::Timeout,
                &["state", "timeoutSeconds"],
            ),
            (
                "cli.wait_disconnected",
                ErrorCategory::Unavailable,
                &["state"],
            ),
            ("cli.interrupted", ErrorCategory::Cancelled, &["state"]),
            (
                "cli.cancel_not_applied",
                ErrorCategory::Conflict,
                &["state"],
            ),
            ("cli.operation_failed", ErrorCategory::Failed, &[]),
            ("cli.operation_cancelled", ErrorCategory::Cancelled, &[]),
            ("cli.operation_needs_recovery", ErrorCategory::Failed, &[]),
            ("cli.instance_not_found", ErrorCategory::NotFound, &["name"]),
            (
                "cli.runtime_not_found",
                ErrorCategory::NotFound,
                &["selector"],
            ),
            (
                "cli.runtime_ambiguous",
                ErrorCategory::Conflict,
                &["selector", "candidates"],
            ),
            ("cli.panic", ErrorCategory::Internal, &[]),
            (
                "cli.agent_root_override",
                ErrorCategory::InvalidInput,
                &["variable"],
            ),
            (
                "cli.agent_not_bundled",
                ErrorCategory::Unsupported,
                &["executable"],
            ),
            (
                "cli.agent_helper_missing",
                ErrorCategory::Unavailable,
                &["bundlePath"],
            ),
            (
                "cli.agent_not_registered",
                ErrorCategory::Unavailable,
                &["label"],
            ),
            (
                "cli.agent_requires_approval",
                ErrorCategory::Unauthorized,
                &["label"],
            ),
            (
                "cli.agent_disabled",
                ErrorCategory::Unauthorized,
                &["label"],
            ),
            (
                "cli.agent_not_found",
                ErrorCategory::Unavailable,
                &["label", "bundlePath"],
            ),
            (
                "cli.agent_invalid_signature",
                ErrorCategory::Unauthorized,
                &["label", "osCode"],
            ),
            (
                "cli.agent_os_error",
                ErrorCategory::Failed,
                &["op", "osDomain", "osCode"],
            ),
            (
                "cli.agent_status_unavailable",
                ErrorCategory::Unavailable,
                &["op", "osCode"],
            ),
            (
                "cli.agent_call_timeout",
                ErrorCategory::Timeout,
                &["op", "seconds"],
            ),
            (
                "cli.daemon_wait_timeout",
                ErrorCategory::Timeout,
                &["action", "seconds"],
            ),
            (
                "cli.connection_not_found",
                ErrorCategory::NotFound,
                &["format", "available"],
            ),
            (
                "cli.connection_unavailable",
                ErrorCategory::Conflict,
                &["format", "reason"],
            ),
        ];
        assert_eq!(table, expected);
        assert!(
            cli::ALL
                .iter()
                .all(|spec| !spec.retryable && spec.items.is_none())
        );
    }

    #[test]
    fn catalog_codes_match_the_spec_0017_table() {
        let table: Vec<_> = catalog::ALL
            .iter()
            .map(|spec| (spec.code, spec.category, spec.details))
            .collect();
        let expected: &[(&str, ErrorCategory, &[&str])] = &[
            (
                "catalog.unavailable",
                ErrorCategory::Unavailable,
                &["reason"],
            ),
            (
                "catalog.provider_not_found",
                ErrorCategory::NotFound,
                &["provider", "suggestion"],
            ),
            (
                "catalog.version_not_found",
                ErrorCategory::NotFound,
                &["provider", "version"],
            ),
            (
                "catalog.not_installable",
                ErrorCategory::Unsupported,
                &[
                    "provider",
                    "version",
                    "target",
                    "availableTargets",
                    "reason",
                ],
            ),
            (
                "catalog.pin_failed",
                ErrorCategory::Failed,
                &["provider", "revision", "reason"],
            ),
        ];
        assert_eq!(table, expected);
        assert!(
            catalog::ALL
                .iter()
                .all(|spec| !spec.retryable && spec.items.is_none())
        );
    }

    #[test]
    fn runtime_codes_match_the_spec_0018_table() {
        use ErrorCategory::{
            Conflict, Failed, InvalidInput, NotFound, Timeout, Unavailable, Unsupported,
        };
        let table: Vec<_> = runtime::ALL
            .iter()
            .map(|spec| (spec.code, spec.category, spec.details))
            .collect();
        let expected: &[(&str, ErrorCategory, &[&str])] = &[
            (
                "runtime.selector_unresolved",
                NotFound,
                &["provider", "selector", "reason", "knownLines"],
            ),
            (
                "runtime.selector_ambiguous",
                InvalidInput,
                &["selector", "versions"],
            ),
            (
                "runtime.selector_changed",
                Conflict,
                &["expectedSha256", "resolvedSha256"],
            ),
            (
                "runtime.already_registered",
                Conflict,
                &["runtimeId", "state"],
            ),
            (
                "runtime.download_offline",
                Unavailable,
                &["artifact", "hosts"],
            ),
            (
                "runtime.download_failed",
                Failed,
                &["artifact", "host", "status", "reason"],
            ),
            (
                "runtime.download_untrusted",
                Failed,
                &["artifact", "host", "reason"],
            ),
            (
                "runtime.download_too_large",
                Failed,
                &["artifact", "expected", "received"],
            ),
            (
                "runtime.digest_mismatch",
                Failed,
                &["artifact", "expected", "actual"],
            ),
            (
                "runtime.size_mismatch",
                Failed,
                &["artifact", "expected", "actual"],
            ),
            (
                "runtime.format_mismatch",
                Failed,
                &["artifact", "format", "kind"],
            ),
            (
                "runtime.insufficient_space",
                Failed,
                &["root", "needed", "available"],
            ),
            ("runtime.io_failed", Failed, &["op", "root"]),
            (
                "runtime.archive_rejected",
                Failed,
                &["artifact", "reason", "entry"],
            ),
            (
                "runtime.architecture_mismatch",
                Unsupported,
                &["path", "arches"],
            ),
            (
                "runtime.os_too_old",
                Unsupported,
                &["path", "required", "host"],
            ),
            ("runtime.executable_missing", Failed, &["path", "reason"]),
            (
                "runtime.dependency_outside_runtime",
                Failed,
                &["path", "library"],
            ),
            (
                "runtime.dependency_mismatch",
                Failed,
                &["missing", "unexpected"],
            ),
            (
                "runtime.signature_invalid",
                Failed,
                &["path", "policy", "detail"],
            ),
            ("runtime.quarantined", Failed, &["path"]),
            ("runtime.binary_rejected_by_os", Failed, &["path", "signal"]),
            (
                "runtime.binary_check_timeout",
                Timeout,
                &["path", "seconds"],
            ),
            (
                "runtime.binary_check_failed",
                Failed,
                &["path", "exitCode", "signal", "errno"],
            ),
            (
                "runtime.version_mismatch",
                Failed,
                &["expected", "observed"],
            ),
            ("runtime.install_conflict", Conflict, &["path"]),
        ];
        assert_eq!(table, expected);
        assert!(
            runtime::ALL
                .iter()
                .all(|spec| !spec.retryable && spec.items.is_none())
        );
    }

    #[test]
    fn every_runtime_code_has_a_short_static_hint_and_no_other_code_has_one() {
        for spec in runtime::ALL {
            let hint = runtime::hint(spec.code, &[]).expect("every runtime code has a hint");
            assert!(hint.len() <= 200, "{}: {hint}", spec.code);
            assert!(!hint.contains('{'), "{}: {hint}", spec.code);
        }
        assert_eq!(runtime::hint("catalog.unavailable", &[]), None);
        assert_eq!(runtime::hint("runtime.from_the_future", &[]), None);
        assert_eq!(
            runtime::hint("runtime.architecture_mismatch", &[]),
            Some(
                "This artifact is not built for Apple Silicon, and Stackly does not run it under Rosetta."
            )
        );
    }

    fn catalog_hint(code: &ErrorSpec, details: &[(&str, &str)]) -> Option<&'static str> {
        let details: Vec<_> = details
            .iter()
            .map(|(key, value)| crate::error::ErrorDetail::new(*key, *value))
            .collect();
        catalog::hint(code.code, &details)
    }

    #[test]
    fn every_catalog_error_has_a_short_static_hint() {
        let mut hints = Vec::new();
        for reason in [
            "not_configured",
            "missing",
            "index_invalid",
            "unsupported_schema",
        ] {
            hints.push(catalog_hint(&catalog::UNAVAILABLE, &[("reason", reason)]));
        }
        hints.push(catalog_hint(
            &catalog::PROVIDER_NOT_FOUND,
            &[("provider", "nats")],
        ));
        hints.push(catalog_hint(
            &catalog::PROVIDER_NOT_FOUND,
            &[("provider", "nats"), ("suggestion", "official/nats")],
        ));
        hints.push(catalog_hint(&catalog::VERSION_NOT_FOUND, &[]));
        hints.push(catalog_hint(
            &catalog::PIN_FAILED,
            &[("reason", "existing_differs")],
        ));
        for reason in [
            "provider_planned",
            "package_invalid",
            "no_build_for_host",
            "no_build_for_arch",
            "artifact_blocked",
            "fixture_only",
            "requires_newer_os",
        ] {
            hints.push(catalog_hint(
                &catalog::NOT_INSTALLABLE,
                &[("reason", reason)],
            ));
        }
        for hint in hints {
            let hint = hint.expect("every catalog case has a hint");
            assert!(hint.len() <= 200, "{hint}");
            assert!(!hint.contains('{'), "{hint}");
        }
    }

    #[test]
    fn a_catalog_hint_depends_on_the_details() {
        assert_eq!(
            catalog_hint(
                &catalog::PROVIDER_NOT_FOUND,
                &[("suggestion", "official/nats")]
            ),
            Some("use the full provider ID, including its namespace")
        );
        assert_eq!(
            catalog_hint(&catalog::PROVIDER_NOT_FOUND, &[]),
            Some("run stackly provider list to see the catalog")
        );
        assert_eq!(
            catalog_hint(&catalog::UNAVAILABLE, &[("reason", "not_configured")]),
            Some("run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR")
        );
        assert_eq!(
            catalog_hint(&catalog::UNAVAILABLE, &[("reason", "from_the_future")]),
            None
        );
        assert_eq!(catalog_hint(&catalog::NOT_INSTALLABLE, &[]), None);
        assert_eq!(catalog_hint(&operation::QUEUE_FULL, &[]), None);
    }

    fn instance_hint(code: &ErrorSpec, details: &[(&str, &str)]) -> Option<&'static str> {
        let details: Vec<_> = details
            .iter()
            .map(|(key, value)| crate::error::ErrorDetail::new(*key, *value))
            .collect();
        instance::hint(code.code, &details)
    }

    #[test]
    fn the_nine_spec_0020_instance_codes_have_a_short_static_hint() {
        for spec in [
            &instance::NAME_INVALID,
            &instance::CAPABILITY_UNSUPPORTED,
            &instance::RUNTIME_INCOMPATIBLE,
            &instance::STORAGE_UNSUPPORTED,
            &instance::INSUFFICIENT_SPACE,
            &instance::DATA_NOT_EMPTY,
            &instance::DATA_OVERLAP,
            &instance::SHORT_KEY_TAKEN,
        ] {
            let hint = instance_hint(spec, &[]).expect("a spec 0020 code has a hint");
            assert!(hint.len() <= 200, "{}: {hint}", spec.code);
            assert!(!hint.contains('{'), "{}: {hint}", spec.code);
        }
        assert_eq!(
            instance_hint(&instance::PROVIDER_UNAVAILABLE, &[("state", "planned")]),
            Some("this provider is not in this release")
        );
        assert_eq!(
            instance_hint(&instance::PROVIDER_UNAVAILABLE, &[("state", "invalid")]),
            Some("reinstall Stackly, this provider package failed validation")
        );
        assert_eq!(
            instance_hint(&instance::PROVIDER_UNAVAILABLE, &[("state", "later")]),
            None
        );
        assert_eq!(instance_hint(&instance::NAME_TAKEN, &[]), None);
    }

    #[test]
    fn the_data_usage_code_has_one_static_hint_per_reason() {
        for (reason, hint) in [
            ("missing", "run stackly doctor; the data folder is missing"),
            (
                "not_private",
                "run stackly doctor; the data folder isn't a private folder you own, or part of it can't be read",
            ),
            ("io", "try again"),
        ] {
            let found = instance_hint(&instance::DATA_USAGE_UNAVAILABLE, &[("reason", reason)]);
            assert_eq!(found, Some(hint));
            assert!(hint.len() <= 200 && !hint.contains('{'));
        }
        assert_eq!(
            instance_hint(&instance::DATA_USAGE_UNAVAILABLE, &[("reason", "later")]),
            None
        );
        assert_eq!(instance_hint(&instance::DATA_USAGE_UNAVAILABLE, &[]), None);
    }

    #[test]
    fn spec_0020_instance_codes_match_the_table() {
        let table: Vec<_> = instance::ALL
            .iter()
            .skip(7)
            .map(|spec| (spec.code, spec.category, spec.details))
            .collect();
        let expected: &[(&str, ErrorCategory, &[&str])] = &[
            (
                "instance.name_invalid",
                ErrorCategory::InvalidInput,
                &["omitted"],
            ),
            (
                "instance.provider_unavailable",
                ErrorCategory::Unsupported,
                &["provider", "state"],
            ),
            (
                "instance.capability_unsupported",
                ErrorCategory::Unsupported,
                &["capability", "provider", "existing"],
            ),
            (
                "instance.runtime_incompatible",
                ErrorCategory::Conflict,
                &["runtimeId", "executable"],
            ),
            (
                "instance.storage_unsupported",
                ErrorCategory::Unsupported,
                &["fsType", "root"],
            ),
            (
                "instance.insufficient_space",
                ErrorCategory::Unavailable,
                &["needed", "available"],
            ),
            (
                "instance.data_not_empty",
                ErrorCategory::Conflict,
                &["path"],
            ),
            (
                "instance.data_overlap",
                ErrorCategory::Conflict,
                &["path", "otherInstance"],
            ),
            (
                "instance.short_key_taken",
                ErrorCategory::Conflict,
                &["shortKey"],
            ),
            (
                "instance.data_usage_unavailable",
                ErrorCategory::Unavailable,
                &["reason"],
            ),
        ];
        assert_eq!(table, expected);
        assert!(instance::ALL.iter().all(|spec| !spec.retryable));
    }

    fn supervisor_hint(code: &ErrorSpec, outcome: Option<&str>) -> Option<&'static str> {
        let details: Vec<_> = outcome
            .map(|outcome| crate::error::ErrorDetail::new("outcome", outcome))
            .into_iter()
            .collect();
        supervisor::hint(code.code, &details)
    }

    #[test]
    fn supervisor_hint_names_the_migration() {
        let incompatible = supervisor_hint(&supervisor::DATA_INCOMPATIBLE, Some("incompatible"))
            .expect("an incompatible line has a hint");
        assert_eq!(
            incompatible,
            "This data belongs to another major version, so this runtime never runs it. Moving it needs a migration: create an instance on this runtime and move the data with a dump and restore."
        );
        let marker = "The version marker in the data folder is missing, unreadable, or not what Stackly recorded, so Stackly can’t tell which runtime may run this data and won’t run it until the marker matches.";
        for outcome in ["marker_missing", "marker_mismatch", "marker_unreadable"] {
            assert_eq!(
                supervisor_hint(&supervisor::DATA_INCOMPATIBLE, Some(outcome)),
                Some(marker),
                "{outcome}"
            );
        }
        for hint in [incompatible, marker] {
            assert!(hint.len() <= 200, "{} bytes: {hint}", hint.len());
            assert!(!hint.contains('{'), "{hint}");
            assert!(hint.is_ascii() || hint.contains('’'), "{hint}");
        }
    }

    #[test]
    fn supervisor_hint_stays_silent_without_a_known_outcome() {
        for outcome in [Some("unknown"), Some("from_the_future"), Some(""), None] {
            assert_eq!(
                supervisor_hint(&supervisor::DATA_INCOMPATIBLE, outcome),
                None,
                "{outcome:?}"
            );
        }
        assert_eq!(
            supervisor_hint(&supervisor::DATA_UNAVAILABLE, Some("incompatible")),
            None
        );
        assert_eq!(supervisor::hint("supervisor.from_the_future", &[]), None);
    }

    #[test]
    fn registry_codes_match_the_spec_0005_wire_table() {
        let table: Vec<_> = registry::ALL
            .iter()
            .map(|spec| (spec.code, spec.category, spec.details))
            .collect();
        let expected: &[(&str, ErrorCategory, &[&str])] = &[
            ("registry.missing_reference", ErrorCategory::Conflict, &[]),
            ("registry.duplicate_resource", ErrorCategory::Conflict, &[]),
            (
                "registry.generation_conflict",
                ErrorCategory::Conflict,
                &["expected", "actual"],
            ),
            (
                "registry.name_taken",
                ErrorCategory::Conflict,
                &["name", "existing"],
            ),
            (
                "registry.short_key_taken",
                ErrorCategory::Conflict,
                &["key", "existing"],
            ),
            (
                "registry.path_taken",
                ErrorCategory::Conflict,
                &["root", "path", "existing"],
            ),
            (
                "registry.duplicate_runtime",
                ErrorCategory::Conflict,
                &["existing"],
            ),
            (
                "registry.dedicated_owner_exists",
                ErrorCategory::Conflict,
                &["instance", "environment"],
            ),
            (
                "registry.active_lease_exists",
                ErrorCategory::Conflict,
                &["instance", "environment"],
            ),
            (
                "registry.software_mismatch",
                ErrorCategory::InvalidInput,
                &["instance", "runtime", "providerRevision"],
            ),
            (
                "registry.path_not_utf8",
                ErrorCategory::InvalidInput,
                &["field"],
            ),
            ("registry.busy", ErrorCategory::Unavailable, &[]),
            ("registry.unavailable", ErrorCategory::Unavailable, &[]),
            ("registry.no_space", ErrorCategory::Failed, &[]),
            ("registry.io_error", ErrorCategory::Failed, &[]),
            (
                "registry.corrupt",
                ErrorCategory::Internal,
                &["table", "column"],
            ),
            ("registry.integrity", ErrorCategory::Internal, &[]),
            (
                "registry.recovery_mode",
                ErrorCategory::Unavailable,
                &["reason", "reportPath"],
            ),
            (
                "registry.not_deletable",
                ErrorCategory::Conflict,
                &["reason"],
            ),
        ];
        assert_eq!(table, expected);
        assert!(registry::ALL.iter().all(|spec| !spec.retryable));
    }

    #[test]
    fn operation_codes_match_the_spec_0007_table() {
        let table: Vec<_> = operation::ALL
            .iter()
            .map(|spec| (spec.code, spec.category, spec.retryable, spec.details))
            .collect();
        let expected: &[(&str, ErrorCategory, bool, &[&str])] = &[
            (
                "operation.idempotency_key_required",
                ErrorCategory::InvalidInput,
                false,
                &["method"],
            ),
            (
                "operation.idempotency_key_reused",
                ErrorCategory::InvalidInput,
                false,
                &["operationId", "method"],
            ),
            (
                "operation.queue_full",
                ErrorCategory::Unavailable,
                true,
                &["max"],
            ),
            (
                "operation.recovery_required",
                ErrorCategory::Conflict,
                false,
                &["operationId", "lockKey"],
            ),
            (
                "operation.not_resumable",
                ErrorCategory::Conflict,
                false,
                &["operationId", "reason"],
            ),
            (
                "operation.not_abandonable",
                ErrorCategory::Conflict,
                false,
                &["operationId", "state", "reason"],
            ),
            (
                "operation.payload_too_large",
                ErrorCategory::Internal,
                false,
                &["field", "max"],
            ),
            (
                "operation.journal_full",
                ErrorCategory::Internal,
                false,
                &["kind", "max"],
            ),
            ("operation.interrupted", ErrorCategory::Failed, false, &[]),
            (
                "operation.daemon_stopping",
                ErrorCategory::Unavailable,
                false,
                &[],
            ),
            (
                "operation.abandoned",
                ErrorCategory::Failed,
                false,
                &["leftovers"],
            ),
            (
                "operation.compensation_failed",
                ErrorCategory::Failed,
                false,
                &["item", "detail"],
            ),
        ];
        assert_eq!(table, expected);
        assert!(operation::ALL.iter().all(|spec| spec.items.is_none()));
    }

    #[test]
    fn manifest_and_provider_codes_match_the_spec_0009_table() {
        let issue = Some(ItemSpec {
            kind: "issue",
            fields: &["file", "location", "code", "message", "reason"],
            max: 100,
        });
        let table: Vec<_> = manifest::ALL
            .iter()
            .chain(provider::ALL)
            .map(|spec| (spec.code, spec.category, spec.details, spec.items))
            .collect();
        let expected: &[(&str, ErrorCategory, &[&str], Option<ItemSpec>)] = &[
            (
                "manifest.invalid",
                ErrorCategory::InvalidInput,
                &["package", "issueCount", "omitted"],
                issue,
            ),
            (
                "provider.invalid_settings",
                ErrorCategory::InvalidInput,
                &["issueCount", "omitted"],
                issue,
            ),
            (
                "provider.unsupported_capability",
                ErrorCategory::Unsupported,
                &["capability"],
                None,
            ),
            (
                "provider.endpoint_not_loopback",
                ErrorCategory::InvalidInput,
                &["endpoint"],
                None,
            ),
            (
                "provider.unrepresentable_value",
                ErrorCategory::InvalidInput,
                &["field"],
                None,
            ),
            (
                "provider.plan_invalid",
                ErrorCategory::InvalidInput,
                &["position", "reason"],
                None,
            ),
            (
                "provider.render_failed",
                ErrorCategory::Failed,
                &["configFile", "reason"],
                None,
            ),
        ];
        assert_eq!(table, expected);
        assert!(
            manifest::ALL
                .iter()
                .chain(provider::ALL)
                .all(|spec| !spec.retryable)
        );
    }

    #[test]
    fn operation_refusal_reasons_are_snake_case_and_distinct() {
        let resume: HashSet<_> = NotResumableReason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(resume.len(), NotResumableReason::ALL.len());
        let abandon: HashSet<_> = NotAbandonableReason::ALL
            .iter()
            .map(|r| r.as_str())
            .collect();
        assert_eq!(abandon.len(), NotAbandonableReason::ALL.len());
        assert!(resume.iter().chain(&abandon).all(|value| is_snake(value)));
    }

    #[test]
    fn provider_reasons_are_snake_case_and_distinct() {
        let plan: HashSet<_> = PlanInvalidReason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(plan.len(), PlanInvalidReason::ALL.len());
        let render: HashSet<_> = RenderFailedReason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(render.len(), RenderFailedReason::ALL.len());
        assert!(plan.iter().chain(&render).all(|value| is_snake(value)));
    }

    #[test]
    fn untrusted_reasons_are_snake_case_and_distinct() {
        let values: HashSet<_> = UntrustedReason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(values.len(), UntrustedReason::ALL.len());
        assert!(values.iter().all(|value| is_snake(value)));
    }
}
