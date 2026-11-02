//! The catalog codes of config changes (spec 0029, *Kode error baru*).
//! `stacklyd` looks each one up in the catalog of `stackly-protocol`.

use stackly_domain::ErrorCodeName;

macro_rules! codes {
    ($($(#[$doc:meta])* $name:ident = $code:literal;)*) => {
        $($(#[$doc])* pub const $name: ErrorCodeName = ErrorCodeName::from_static($code);)*

        /// Every code above, for the catalog test of `stackly-protocol`.
        pub const ALL: &[ErrorCodeName] = &[$($name),*];
    };
}

codes! {
    /// `baseRevision` is not the active revision.
    REVISION_CONFLICT = "config.revision_conflict";
    /// A `configDrift` blocker is on the instance, or a render found drift.
    DRIFT_DETECTED = "config.drift_detected";
    /// A roll back without a last known good other than the active one.
    NO_LAST_KNOWN_GOOD = "config.no_last_known_good";
}
