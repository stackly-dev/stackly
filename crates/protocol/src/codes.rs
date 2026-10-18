//! The error code catalog, one submodule per area (VAL-020).
//!
//! Every code on the wire is `<area>.<reason>` in snake case and lives here
//! with its category, its `retryable` flag, and the `details` keys it may
//! carry. Each spec owns its areas and adds codes only there, so specs
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
    /// When the code is used, in one English sentence.
    pub summary: &'static str,
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

/// Defines one area submodule: a constant per code and the `ALL` slice.
macro_rules! area {
    (
        $(#[$meta:meta])*
        pub mod $module:ident = $area:literal, owner $owner:literal {
            $(
                $konst:ident = $code:literal, $category:ident, retryable $retryable:literal,
                details [$($key:literal),* $(,)?], $summary:literal;
            )*
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
                    summary: $summary,
                };
            )*

            /// Every code in this area.
            pub const ALL: &[ErrorSpec] = &[$($konst),*];

            /// This area, as registered in [`AREAS`](super::AREAS).
            pub const AREA: Area = Area { name: NAME, owner: $owner, codes: ALL };
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
            details [], "client side: the daemon closed the connection before replying, so the request may have run";
        IO_ERROR = "ipc.io_error", Unavailable, retryable true,
            details ["op", "kind"], "client side: another os error while checking the socket path or connecting";
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
            details ["method"], "client side: the client stopped waiting for a reply, so the request may have run";
    }
}

area! {
    /// Request decoding and request bookkeeping codes.
    pub mod request = "request", owner "spec 0006" {
        MALFORMED = "request.malformed", InvalidInput, retryable false,
            details [], "the json is broken, the request shape is wrong, or requestId or idempotencyKey has a bad format";
        TOO_DEEP = "request.too_deep", InvalidInput, retryable false,
            details ["max"], "the json nests deeper than the limit";
        UNKNOWN_METHOD = "request.unknown_method", Unsupported, retryable false,
            details ["method"], "the daemon does not know the method";
        INVALID_PARAMS = "request.invalid_params", InvalidInput, retryable false,
            details ["method"], "the params fail to decode or to validate, and fields lists each one";
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
        LOCK_HELD = "daemon.lock_held", Conflict, retryable false,
            details ["pid", "serverInstanceId", "version", "startedAt"], "another daemon holds the lock, only ever printed to stderr";
    }
}

area! {
    /// Codes for bugs and broken invariants.
    pub mod internal = "internal", owner "spec 0006" {
        UNEXPECTED = "internal.unexpected", Internal, retryable false,
            details [], "a handler panicked or an invariant broke";
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
    internal::AREA,
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
    fn spec_0006_owns_every_area_it_registers() {
        let names: Vec<_> = AREAS.iter().map(|area| area.name).collect();
        assert_eq!(
            names,
            [
                "protocol", "ipc", "request", "events", "resource", "daemon", "internal"
            ]
        );
        assert!(AREAS.iter().all(|area| area.owner == "spec 0006"));
    }

    #[test]
    fn untrusted_reasons_are_snake_case_and_distinct() {
        let values: HashSet<_> = UntrustedReason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(values.len(), UntrustedReason::ALL.len());
        assert!(values.iter().all(|value| is_snake(value)));
    }
}
