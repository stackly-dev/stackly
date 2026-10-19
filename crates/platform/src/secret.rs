//! The secret store port (spec 0011): [`SecretStore`], the value type
//! [`SecretValue`] that hides itself, and the vocabulary every adapter
//! shares.
//!
//! A secret value lives only in a [`SecretValue`]: its `Debug` prints
//! `SecretValue(****)`, it has no `Display`, `Clone`, `PartialEq`, or serde
//! support, and its memory is wiped when it drops. The only way to read it
//! is [`SecretValue::expose`], which `cargo xtask check-deps` allows only in
//! a short list of modules.

use std::fmt;
use std::mem;

use stackly_domain::{Os, SecretRef};
use zeroize::Zeroizing;

/// The longest secret value, in bytes: room for a PEM private key with its
/// certificate chain.
pub const MAX_SECRET_LEN: usize = 65_536;
/// The shortest value the redactor learns, in bytes. A shorter value is
/// accepted, but only its type and the key patterns hide it.
pub const SCRUB_MIN_LEN: usize = 8;
/// The shortest value the redactor replaces anywhere it appears, and that
/// may not appear inside an argument, in bytes. A value from
/// [`SCRUB_MIN_LEN`] up to this length is only replaced as a whole token.
pub const SCRUB_ANYWHERE_LEN: usize = 16;
/// The service every Stackly secret is stored under, next to the
/// reference path as the account.
pub const SECRET_SERVICE: &str = "stackly-secret";

/// Why a text cannot be a [`SecretValue`]. The message never holds the
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SecretValueError {
    /// The value is empty.
    #[error("a secret value must not be empty")]
    Empty,
    /// The value is longer than [`MAX_SECRET_LEN`] bytes.
    #[error("a secret value must be at most 65536 bytes")]
    TooLong,
    /// The value holds a NUL byte.
    #[error("a secret value must not contain a NUL byte")]
    ContainsNul,
    /// The bytes are not UTF-8.
    #[error("a secret value must be UTF-8")]
    NotUtf8,
}

/// A secret value in memory: UTF-8, 1 to [`MAX_SECRET_LEN`] bytes, with no
/// NUL byte (AC-7).
///
/// Its memory is wiped when it drops, and its `Debug` never prints it.
///
/// ```
/// use stackly_platform::SecretValue;
///
/// let value = SecretValue::new("hunter2-but-longer".to_owned())?;
/// assert_eq!(format!("{value:?}"), "SecretValue(****)");
/// assert_eq!(value.len(), 18);
/// # Ok::<(), stackly_platform::SecretValueError>(())
/// ```
pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    /// Takes ownership of a value. On error the text is wiped before it
    /// drops.
    ///
    /// # Errors
    ///
    /// Returns a [`SecretValueError`] for an empty value, one longer than
    /// [`MAX_SECRET_LEN`] bytes, or one that holds a NUL byte.
    pub fn new(value: String) -> Result<Self, SecretValueError> {
        let value = Zeroizing::new(value);
        if value.is_empty() {
            return Err(SecretValueError::Empty);
        }
        if value.len() > MAX_SECRET_LEN {
            return Err(SecretValueError::TooLong);
        }
        if value.contains('\0') {
            return Err(SecretValueError::ContainsNul);
        }
        Ok(Self(value))
    }

    /// Takes ownership of bytes read from a store or a file, without
    /// copying them. On error the bytes are wiped before they drop.
    ///
    /// # Errors
    ///
    /// Returns [`SecretValueError::NotUtf8`] when the bytes are not UTF-8,
    /// and the errors of [`SecretValue::new`] otherwise.
    pub fn from_utf8(bytes: Vec<u8>) -> Result<Self, SecretValueError> {
        let mut bytes = Zeroizing::new(bytes);
        if std::str::from_utf8(&bytes).is_err() {
            return Err(SecretValueError::NotUtf8);
        }
        let text =
            String::from_utf8(mem::take(&mut *bytes)).map_err(|_| SecretValueError::NotUtf8)?;
        Self::new(text)
    }

    /// Returns the value. Only the modules on the `check-deps` allowlist
    /// may call it (spec 0011, *Penjaga di kode*).
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Returns the length in bytes, for example to compare it with
    /// [`SCRUB_MIN_LEN`], without exposing the value.
    #[expect(clippy::len_without_is_empty, reason = "a secret value is never empty")]
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(****)")
    }
}

/// Defines a fieldless enum with a stable snake_case code, `as_code()`,
/// and `Display` through the code.
macro_rules! secret_code_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $code:literal,
            )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )+
        }

        impl $name {
            /// Every value, in code order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Returns the stable snake_case code.
            #[must_use]
            pub const fn as_code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_code())
            }
        }
    };
}

secret_code_enum! {
    /// Whether a store call may show the user a dialog (AC-4).
    pub enum Interaction {
        /// A user action from an interactive client: an unlock or access
        /// dialog may appear for the length of this one call.
        AllowPrompt => "allow_prompt",
        /// A background trigger or a noninteractive client: no dialog ever
        /// appears, and the call fails instead.
        Never => "never",
    }
}

secret_code_enum! {
    /// How [`SecretStore::write`] treats an item that does or does not
    /// exist yet.
    pub enum WriteMode {
        /// Fails with [`SecretStoreError::AlreadyExists`] when an item
        /// exists.
        Create => "create",
        /// Fails with [`SecretStoreError::NotFound`] when no item exists.
        Replace => "replace",
        /// Creates or replaces.
        Upsert => "upsert",
    }
}

secret_code_enum! {
    /// Whether the store holds an item for a reference, read from its
    /// attributes alone.
    pub enum SecretPresence {
        /// An item exists.
        Present => "present",
        /// No item exists.
        Missing => "missing",
    }
}

secret_code_enum! {
    /// The state of the store as last observed, which
    /// [`SecretStore::state`] reads from a cache.
    pub enum StoreState {
        /// The last call reached the store.
        Available => "available",
        /// The last call found the store locked.
        Locked => "locked",
        /// The store is not supported on this OS.
        Unsupported => "unsupported",
        /// No call has observed the store yet.
        Unknown => "unknown",
    }
}

secret_code_enum! {
    /// Which store an adapter talks to.
    pub enum SecretBackend {
        /// The macOS Keychain.
        MacosKeychain => "macos_keychain",
        /// The in-memory fake of `stackly-testkit`.
        Fake => "fake",
        /// No store: [`UnsupportedSecretStore`].
        Unsupported => "unsupported",
    }
}

/// Error returned by a [`SecretStore`]. No message ever holds a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SecretStoreError {
    /// The store holds no item for the reference.
    #[error("the secret store holds no value for this reference")]
    NotFound,
    /// The store already holds an item for the reference.
    #[error("the secret store already holds a value for this reference")]
    AlreadyExists,
    /// The store is locked.
    #[error("the secret store is locked")]
    Locked,
    /// The store refused access: the user denied or cancelled a dialog
    /// (`by_user`), or the item does not trust this process and no dialog
    /// was allowed.
    #[error("access to the secret was denied {}", if *by_user { "by the user" } else { "because the item does not trust this process" })]
    AccessDenied {
        /// True when the user denied or cancelled a dialog.
        by_user: bool,
    },
    /// The call did not start before its wait limit.
    #[error("the secret store is busy")]
    Busy,
    /// A write or delete started but passed its wait limit, so whether it
    /// took effect is unknown until the next probe.
    #[error("the outcome of the secret store change is unknown")]
    OutcomeUnknown,
    /// The store is not supported on this OS.
    #[error("the secret store is not supported on {os}")]
    NotSupported {
        /// The OS that lacks it.
        os: Os,
    },
    /// Any other store failure, with the backend's status code.
    #[error("the secret store failed with code {code}")]
    Backend {
        /// The status code, for example an `OSStatus` on macOS.
        code: i32,
    },
}

/// The user's secret store, where every secret value lives (spec 0011).
///
/// Only `stacklyd` calls it, through one worker thread. Every method is
/// sync and object safe; an adapter holds its own lock, so it is safe to
/// call from any thread.
pub trait SecretStore: Send + Sync {
    /// Returns which store this is.
    fn backend(&self) -> SecretBackend;

    /// Returns the state of the store as the last call observed it,
    /// without ever calling the store.
    fn state(&self) -> StoreState;

    /// Returns the key that names the reference's item in this store, for
    /// the registry's `platform_store_key`. It is deterministic and does no
    /// I/O.
    fn store_key(&self, reference: &SecretRef) -> String;

    /// Tells whether an item exists, from its attributes alone and never
    /// with a dialog. A store that can look up attributes while it is
    /// locked, such as the macOS Keychain, still answers then.
    ///
    /// # Errors
    ///
    /// Returns [`SecretStoreError::Locked`] for a locked store that cannot
    /// check, [`SecretStoreError::NotSupported`] without a store, and
    /// [`SecretStoreError::Backend`] for any other failure.
    fn probe(&self, reference: &SecretRef) -> Result<SecretPresence, SecretStoreError>;

    /// Reads the value behind the reference.
    ///
    /// # Errors
    ///
    /// Returns [`SecretStoreError::NotFound`] when no item exists (a
    /// locked Keychain still knows that), [`SecretStoreError::Locked`]
    /// when the store is locked, [`SecretStoreError::AccessDenied`] when
    /// access is refused, and the other variants as their names say.
    fn read(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<SecretValue, SecretStoreError>;

    /// Stores the value behind the reference, atomically on the store's
    /// side.
    ///
    /// # Errors
    ///
    /// Returns [`SecretStoreError::AlreadyExists`] for
    /// [`WriteMode::Create`] on an existing item,
    /// [`SecretStoreError::NotFound`] for [`WriteMode::Replace`] on a
    /// missing one, and the errors of [`read`](Self::read) otherwise.
    fn write(
        &self,
        reference: &SecretRef,
        value: &SecretValue,
        mode: WriteMode,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError>;

    /// Removes the item behind the reference.
    ///
    /// # Errors
    ///
    /// Returns [`SecretStoreError::NotFound`] when no item exists, and the
    /// errors of [`read`](Self::read) otherwise.
    fn delete(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError>;
}

/// The store of an OS without an adapter: every fallible method answers
/// [`SecretStoreError::NotSupported`], and the state is always
/// [`StoreState::Unsupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedSecretStore {
    os: Os,
}

impl UnsupportedSecretStore {
    /// Creates the store for `os`.
    #[must_use]
    pub const fn new(os: Os) -> Self {
        Self { os }
    }

    const fn not_supported(&self) -> SecretStoreError {
        SecretStoreError::NotSupported { os: self.os }
    }
}

impl SecretStore for UnsupportedSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::Unsupported
    }

    fn state(&self) -> StoreState {
        StoreState::Unsupported
    }

    fn store_key(&self, reference: &SecretRef) -> String {
        format!("unsupported:{SECRET_SERVICE}/{}", reference.path())
    }

    fn probe(&self, _reference: &SecretRef) -> Result<SecretPresence, SecretStoreError> {
        Err(self.not_supported())
    }

    fn read(
        &self,
        _reference: &SecretRef,
        _interaction: Interaction,
    ) -> Result<SecretValue, SecretStoreError> {
        Err(self.not_supported())
    }

    fn write(
        &self,
        _reference: &SecretRef,
        _value: &SecretValue,
        _mode: WriteMode,
        _interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        Err(self.not_supported())
    }

    fn delete(
        &self,
        _reference: &SecretRef,
        _interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        Err(self.not_supported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "stackly-fixture-credential-0123456789abcdef";

    #[test]
    fn a_value_hides_itself_in_debug_and_reads_back_through_expose() {
        let value = SecretValue::new(FIXTURE.to_owned()).unwrap();

        assert_eq!(format!("{value:?}"), "SecretValue(****)");
        assert_eq!(format!("{value:#?}"), "SecretValue(****)");
        assert_eq!(value.expose(), FIXTURE);
        assert_eq!(value.len(), FIXTURE.len());
    }

    #[test]
    fn a_value_is_one_to_65536_bytes_of_utf8_without_nul() {
        let longest = "a".repeat(MAX_SECRET_LEN);
        assert_eq!(SecretValue::new(longest).unwrap().len(), MAX_SECRET_LEN);
        assert_eq!(SecretValue::new("x".to_owned()).unwrap().len(), 1);
        assert_eq!(SecretValue::new("pässwörd".to_owned()).unwrap().len(), 10);

        let cases = [
            (String::new(), SecretValueError::Empty),
            ("a".repeat(MAX_SECRET_LEN + 1), SecretValueError::TooLong),
            ("pass\0word".to_owned(), SecretValueError::ContainsNul),
        ];
        for (text, expected) in cases {
            assert_eq!(SecretValue::new(text).unwrap_err(), expected);
        }
    }

    #[test]
    fn bytes_become_a_value_only_when_they_are_valid_utf8() {
        let value = SecretValue::from_utf8(FIXTURE.as_bytes().to_vec()).unwrap();
        assert_eq!(value.expose(), FIXTURE);

        assert_eq!(
            SecretValue::from_utf8(vec![0x66, 0xff, 0x6f]).unwrap_err(),
            SecretValueError::NotUtf8
        );
        assert_eq!(
            SecretValue::from_utf8(Vec::new()).unwrap_err(),
            SecretValueError::Empty
        );
        assert_eq!(
            SecretValue::from_utf8(b"a\0b".to_vec()).unwrap_err(),
            SecretValueError::ContainsNul
        );
    }

    #[test]
    fn errors_never_hold_the_value() {
        for err in [
            SecretValue::new(format!("{FIXTURE}\0")).unwrap_err(),
            SecretValue::new(FIXTURE.repeat(2000)).unwrap_err(),
        ] {
            assert!(!err.to_string().contains(FIXTURE), "{err}");
            assert!(!format!("{err:?}").contains(FIXTURE), "{err:?}");
        }
    }

    #[test]
    fn the_scrub_thresholds_are_8_and_16_bytes() {
        assert_eq!(SCRUB_MIN_LEN, 8);
        assert_eq!(SCRUB_ANYWHERE_LEN, 16);
        assert_eq!(MAX_SECRET_LEN, 65_536);
    }

    #[test]
    fn store_codes_are_stable_snake_case() {
        let codes = |codes: Vec<&'static str>| codes.join(",");
        assert_eq!(
            codes(Interaction::ALL.iter().map(|v| v.as_code()).collect()),
            "allow_prompt,never"
        );
        assert_eq!(
            codes(WriteMode::ALL.iter().map(|v| v.as_code()).collect()),
            "create,replace,upsert"
        );
        assert_eq!(
            codes(SecretPresence::ALL.iter().map(|v| v.as_code()).collect()),
            "present,missing"
        );
        assert_eq!(
            codes(StoreState::ALL.iter().map(|v| v.as_code()).collect()),
            "available,locked,unsupported,unknown"
        );
        assert_eq!(
            codes(SecretBackend::ALL.iter().map(|v| v.as_code()).collect()),
            "macos_keychain,fake,unsupported"
        );
        assert_eq!(StoreState::Locked.to_string(), "locked");
    }

    #[test]
    fn store_errors_read_as_lowercase_sentences() {
        let cases = [
            (
                SecretStoreError::NotFound,
                "the secret store holds no value for this reference",
            ),
            (
                SecretStoreError::AlreadyExists,
                "the secret store already holds a value for this reference",
            ),
            (SecretStoreError::Locked, "the secret store is locked"),
            (
                SecretStoreError::AccessDenied { by_user: true },
                "access to the secret was denied by the user",
            ),
            (
                SecretStoreError::AccessDenied { by_user: false },
                "access to the secret was denied because the item does not trust this process",
            ),
            (SecretStoreError::Busy, "the secret store is busy"),
            (
                SecretStoreError::OutcomeUnknown,
                "the outcome of the secret store change is unknown",
            ),
            (
                SecretStoreError::NotSupported { os: Os::Windows },
                "the secret store is not supported on windows",
            ),
            (
                SecretStoreError::Backend { code: -25_308 },
                "the secret store failed with code -25308",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn the_unsupported_store_refuses_every_call_and_reports_unsupported() {
        let store = UnsupportedSecretStore::new(Os::Windows);
        let reference = SecretRef::parse("local:erp/admin").unwrap();
        let value = SecretValue::new(FIXTURE.to_owned()).unwrap();
        let not_supported = SecretStoreError::NotSupported { os: Os::Windows };

        assert_eq!(store.backend(), SecretBackend::Unsupported);
        assert_eq!(store.state(), StoreState::Unsupported);
        assert_eq!(
            store.store_key(&reference),
            "unsupported:stackly-secret/erp/admin"
        );
        assert_eq!(store.probe(&reference), Err(not_supported));
        assert_eq!(
            store.read(&reference, Interaction::Never).unwrap_err(),
            not_supported
        );
        assert_eq!(
            store.write(&reference, &value, WriteMode::Upsert, Interaction::Never),
            Err(not_supported)
        );
        assert_eq!(
            store.delete(&reference, Interaction::AllowPrompt),
            Err(not_supported)
        );
    }
}
