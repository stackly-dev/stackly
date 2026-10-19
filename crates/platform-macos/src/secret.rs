//! The macOS Keychain adapter of the secret store port (spec 0011).
//!
//! Every value is a generic password with service `stackly-secret` and the
//! reference path as its account, in exactly one keychain: the login
//! Keychain for the default root, or a file keychain for a root override
//! and for tests. Security framework user interaction stays off for the
//! whole process except during one `AllowPrompt` call, and one lock for the
//! whole process serializes every Keychain call, so that switch is safe no
//! matter how many stores exist or which thread calls them.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use core_foundation::base::TCFType;
use core_foundation::data::CFData;
use security_framework::item::{
    ItemAddOptions, ItemAddValue, ItemClass, ItemSearchOptions, ItemUpdateOptions, ItemUpdateValue,
    Location, SearchResult, update_item,
};
use security_framework::os::macos::keychain::{
    CreateOptions, KeychainSettings, KeychainUserInteractionLock, SecKeychain,
};
use security_framework::random::SecRandom;
use stackly_domain::SecretRef;
use stackly_platform::{
    FileKind, Filesystem, Interaction, PlatformError, PrivateDirViolation, SECRET_SERVICE,
    SecretBackend, SecretPresence, SecretStore, SecretStoreError, SecretValue, StoreState,
    UserIdentity, WriteMode, check_private_dir,
};

use crate::dirs::home_dir;
use crate::fs::MacosFilesystem;
use crate::identity::MacosIdentity;

/// `errSecUserCanceled`: the user denied or cancelled a dialog.
const ERR_USER_CANCELED: i32 = -128;
/// `errSecAuthFailed`: a locked keychain, or an item whose ACL does not
/// trust this process when no dialog may appear (spec 0011 spike).
const ERR_AUTH_FAILED: i32 = -25_293;
/// `errSecDuplicateItem`.
const ERR_DUPLICATE_ITEM: i32 = -25_299;
/// `errSecItemNotFound`, true even on a locked keychain.
const ERR_ITEM_NOT_FOUND: i32 = -25_300;
/// `errSecInteractionNotAllowed`.
const ERR_INTERACTION_NOT_ALLOWED: i32 = -25_308;
/// `errSecDecode`, reported for an item whose data is not a valid secret
/// value (empty, too long, with a NUL byte, or not UTF-8).
const ERR_DECODE: i32 = -26_275;
/// `kSecUnlockStateStatus`, the bit of `SecKeychainGetStatus` that is set
/// while the keychain is unlocked.
const UNLOCK_STATE_STATUS: u32 = 1;
/// The login Keychain, relative to the home directory.
const LOGIN_KEYCHAIN: &str = "Library/Keychains/login.keychain-db";
/// File name of a root override's keychain.
const FILE_KEYCHAIN_NAME: &str = "stackly.keychain-db";
/// File name of the password of a root override's keychain.
const FILE_PASSWORD_NAME: &str = "stackly.keychain-password";
/// Length of a generated file keychain password.
const FILE_PASSWORD_LEN: usize = 32;
/// The largest password file read back, in bytes.
const FILE_PASSWORD_MAX: u64 = 1024;
/// The characters of a generated file keychain password.
const PASSWORD_ALPHABET: &[u8; 62] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

// SAFETY: this declares the real C signature of `SecKeychainGetStatus`
// from Security.framework (`OSStatus SecKeychainGetStatus(SecKeychainRef,
// SecKeychainStatus *)`, with `SecKeychainStatus` a `UInt32`). Neither
// security-framework nor its -sys crate declares it (spec 0011 spike).
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecKeychainGetStatus(keychain: *mut c_void, status: *mut u32) -> i32;
}

/// Which keychain a [`MacosSecretStore`] uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeychainTarget {
    /// The user's login Keychain, for the default root. It is opened by
    /// path from the home directory in the user database, never created or
    /// unlocked by Stackly.
    Login,
    /// A file keychain of its own, for a root override and for tests. It
    /// is created when missing with a random password kept in
    /// `password_file`, and never added to the user's search list.
    File {
        /// The keychain file, for example `<data>/keychain/stackly.keychain-db`.
        path: PathBuf,
        /// The file that holds its password, in the same folder.
        password_file: PathBuf,
    },
}

impl KeychainTarget {
    /// Returns the file keychain of a root override in `dir`, usually
    /// `DirectoryLayout::keychain_dir`: `stackly.keychain-db` with its
    /// password in `stackly.keychain-password`.
    #[must_use]
    pub fn in_dir(dir: &Path) -> Self {
        Self::File {
            path: dir.join(FILE_KEYCHAIN_NAME),
            password_file: dir.join(FILE_PASSWORD_NAME),
        }
    }

    /// Returns the target part of a store key: `login`, or `file:` with the
    /// keychain's folder and file name, which under the standard layout is
    /// its path relative to the data root (`keychain/stackly.keychain-db`).
    fn key_text(&self) -> String {
        match self {
            Self::Login => "login".to_owned(),
            Self::File { path, .. } => {
                let mut tail: Vec<String> = path
                    .iter()
                    .rev()
                    .take(2)
                    .map(|part| part.to_string_lossy().into_owned())
                    .collect();
                tail.reverse();
                format!("file:{}", tail.join("/"))
            }
        }
    }
}

/// Why a [`MacosSecretStore`] could not be set up. The daemon stops with
/// it at start rather than run with a store it cannot use.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeychainSetupError {
    /// A folder or file could not be prepared, or the home directory is
    /// unknown.
    #[error(transparent)]
    Platform(#[from] PlatformError),
    /// The keychain folder exists but is not a private folder of this
    /// user. It is never repaired.
    #[error("{} is not a private Stackly folder: {violation}", path.display())]
    NotPrivate {
        /// The folder.
        path: PathBuf,
        /// The rule it broke.
        violation: PrivateDirViolation,
    },
    /// The keychain and its password file must share one folder.
    #[error("the keychain {} and its password file {} must be in the same folder", path.display(), password_file.display())]
    SplitFolders {
        /// The keychain file.
        path: PathBuf,
        /// The password file.
        password_file: PathBuf,
    },
    /// The file keychain exists but its password file is missing or does
    /// not hold a password.
    #[error("the keychain {} exists but its password file {} is missing or invalid", path.display(), password_file.display())]
    PasswordFile {
        /// The keychain file.
        path: PathBuf,
        /// The password file.
        password_file: PathBuf,
    },
    /// A Security framework call failed while setting up the keychain.
    #[error("cannot {step} the keychain {}: OSStatus {code}", path.display())]
    Keychain {
        /// What was being done, for example `unlock`.
        step: &'static str,
        /// The keychain file.
        path: PathBuf,
        /// The `OSStatus`.
        code: i32,
    },
}

/// Maps the `OSStatus` of a failed Keychain call to a store error (spec
/// 0011, AC-3), with whether the keychain is locked right after the call
/// and the interaction the call ran with.
///
/// | Code | Result |
/// | --- | --- |
/// | `errSecItemNotFound` (-25300) | `NotFound`, even when locked |
/// | `errSecDuplicateItem` (-25299) | `AlreadyExists` |
/// | `errSecAuthFailed` (-25293) | `Locked` when locked; else `AccessDenied`, `by_user` only under `AllowPrompt` |
/// | `errSecInteractionNotAllowed` (-25308) | `Locked` when locked; else `AccessDenied { by_user: false }` |
/// | `errSecUserCanceled` (-128) | `AccessDenied { by_user: true }` |
/// | any other | `Backend { code }` |
#[must_use]
pub const fn map_status(
    code: i32,
    keychain_locked: bool,
    interaction: Interaction,
) -> SecretStoreError {
    match code {
        ERR_ITEM_NOT_FOUND => SecretStoreError::NotFound,
        ERR_DUPLICATE_ITEM => SecretStoreError::AlreadyExists,
        ERR_AUTH_FAILED | ERR_INTERACTION_NOT_ALLOWED if keychain_locked => {
            SecretStoreError::Locked
        }
        ERR_AUTH_FAILED => SecretStoreError::AccessDenied {
            by_user: matches!(interaction, Interaction::AllowPrompt),
        },
        ERR_INTERACTION_NOT_ALLOWED => SecretStoreError::AccessDenied { by_user: false },
        ERR_USER_CANCELED => SecretStoreError::AccessDenied { by_user: true },
        code => SecretStoreError::Backend { code },
    }
}

/// The Security framework user interaction switch of this process: `Some`
/// while interaction is off. Every Keychain call of every store holds this
/// lock, so the switch never changes under another call.
static INTERACTION: Mutex<Option<KeychainUserInteractionLock>> = Mutex::new(None);

/// Holds the process wide Keychain lock with interaction off.
struct KeychainGuard {
    switch: MutexGuard<'static, Option<KeychainUserInteractionLock>>,
}

impl KeychainGuard {
    /// Takes the lock and makes sure interaction is off.
    fn acquire() -> Result<Self, i32> {
        let mut guard = Self {
            switch: INTERACTION.lock().unwrap_or_else(PoisonError::into_inner),
        };
        guard.disable()?;
        Ok(guard)
    }

    fn disable(&mut self) -> Result<(), i32> {
        if self.switch.is_none() {
            let off = SecKeychain::disable_user_interaction().map_err(|err| err.code())?;
            *self.switch = Some(off);
        }
        Ok(())
    }

    /// Runs `call` with interaction on when `interaction` allows a dialog,
    /// and turns it off again before it returns.
    fn run<T>(
        &mut self,
        interaction: Interaction,
        call: impl FnOnce() -> security_framework::base::Result<T>,
    ) -> Result<T, i32> {
        if interaction == Interaction::Never {
            return call().map_err(|err| err.code());
        }
        // Dropping the lock object turns interaction back on.
        *self.switch = None;
        let result = call().map_err(|err| err.code());
        if self.disable().is_err() {
            // The call already ran, so its own result stands. Interaction
            // stays on only until the next call: `acquire` turns it off
            // again, and refuses to run that call when it cannot.
        }
        result
    }
}

/// Reads whether `keychain` is unlocked, without a dialog.
fn keychain_unlocked(keychain: &SecKeychain) -> Result<bool, i32> {
    let mut status: u32 = 0;
    // SAFETY: `keychain` borrows a live SecKeychainRef that stays retained
    // for the whole call, `status` is a valid, aligned u32 the function
    // writes once, and SecKeychainGetStatus keeps neither pointer.
    let code =
        unsafe { SecKeychainGetStatus(keychain.as_concrete_TypeRef().cast(), &raw mut status) };
    if code == 0 {
        Ok(status & UNLOCK_STATE_STATUS != 0)
    } else {
        Err(code)
    }
}

/// The macOS Keychain behind the [`SecretStore`] port (spec 0011).
///
/// Every call is pinned to one keychain (`kSecUseKeychain` to add,
/// `kSecMatchSearchList` holding only it otherwise). After every call the
/// adapter reads the keychain's lock status, which feeds [`map_status`]
/// and the [`state`](SecretStore::state) cache.
pub struct MacosSecretStore {
    target: KeychainTarget,
    keychain: SecKeychain,
    state: Mutex<StoreState>,
}

impl MacosSecretStore {
    /// Opens the keychain of `target`, creating a file keychain when it is
    /// missing, and turns Security framework user interaction off for the
    /// process.
    ///
    /// A file keychain lives in a private folder this creates with mode
    /// 0700; a new one gets a random 32 character password, written first
    /// to its password file (mode 0600), is set never to lock on its own
    /// or on sleep, and is unlocked once here. It is never added to the
    /// user's search list.
    ///
    /// # Errors
    ///
    /// Returns [`KeychainSetupError::NotPrivate`] for a keychain folder
    /// that is not private, [`KeychainSetupError::PasswordFile`] for an
    /// existing keychain without its password,
    /// [`KeychainSetupError::Keychain`] when a Security framework call
    /// fails, and [`KeychainSetupError::Platform`] when a folder, file, or
    /// the home directory cannot be read or written.
    pub fn new(target: KeychainTarget) -> Result<Self, KeychainSetupError> {
        let guard = KeychainGuard::acquire().map_err(|code| KeychainSetupError::Keychain {
            step: "turn off user interaction for",
            path: keychain_path(&target),
            code,
        })?;
        let keychain = match &target {
            KeychainTarget::Login => {
                let path = home_dir()?.join(LOGIN_KEYCHAIN);
                SecKeychain::open(&path).map_err(|err| KeychainSetupError::Keychain {
                    step: "open",
                    path,
                    code: err.code(),
                })?
            }
            KeychainTarget::File {
                path,
                password_file,
            } => open_file_keychain(path, password_file)?,
        };
        let state = observed_state(&keychain);
        drop(guard);
        Ok(Self {
            target,
            keychain,
            state: Mutex::new(state),
        })
    }

    /// Returns the keychain this store uses.
    #[must_use]
    pub const fn target(&self) -> &KeychainTarget {
        &self.target
    }

    fn search(&self, reference: &SecretRef) -> ItemSearchOptions {
        let mut search = ItemSearchOptions::new();
        search
            .class(ItemClass::generic_password())
            .service(SECRET_SERVICE)
            .account(reference.path())
            .keychains(std::slice::from_ref(&self.keychain));
        search
    }

    /// Runs one Keychain call under the process wide lock, then reads the
    /// lock status for the state cache and for [`map_status`].
    fn call<T>(
        &self,
        interaction: Interaction,
        call: impl FnOnce() -> security_framework::base::Result<T>,
    ) -> Result<T, SecretStoreError> {
        let mut guard =
            KeychainGuard::acquire().map_err(|code| SecretStoreError::Backend { code })?;
        let result = guard.run(interaction, call);
        let state = observed_state(&self.keychain);
        drop(guard);
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = state;
        result.map_err(|code| map_status(code, state == StoreState::Locked, interaction))
    }

    fn add(
        &self,
        reference: &SecretRef,
        value: &SecretValue,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        let data = CFData::from_buffer(value.expose().as_bytes());
        let mut options = ItemAddOptions::new(ItemAddValue::Data {
            class: ItemClass::generic_password(),
            data,
        });
        options
            .set_service(SECRET_SERVICE)
            .set_account_name(reference.path())
            .set_label(format!("Stackly {reference}"))
            .set_description("Stackly secret")
            .set_location(Location::FileKeychain(self.keychain.clone()));
        self.call(interaction, || options.add())
    }

    fn update(
        &self,
        reference: &SecretRef,
        value: &SecretValue,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        let search = self.search(reference);
        let mut changes = ItemUpdateOptions::new();
        changes.set_value(ItemUpdateValue::Data(CFData::from_buffer(
            value.expose().as_bytes(),
        )));
        self.call(interaction, || update_item(&search, &changes))
    }
}

impl std::fmt::Debug for MacosSecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosSecretStore")
            .field("target", &self.target)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl SecretStore for MacosSecretStore {
    fn backend(&self) -> SecretBackend {
        SecretBackend::MacosKeychain
    }

    fn state(&self) -> StoreState {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn store_key(&self, reference: &SecretRef) -> String {
        format!(
            "macos-keychain:{}/{SECRET_SERVICE}/{}",
            self.target.key_text(),
            reference.path()
        )
    }

    fn probe(&self, reference: &SecretRef) -> Result<SecretPresence, SecretStoreError> {
        let mut search = self.search(reference);
        search.load_attributes(true);
        match self.call(Interaction::Never, || search.search()) {
            Ok(_) => Ok(SecretPresence::Present),
            Err(SecretStoreError::NotFound) => Ok(SecretPresence::Missing),
            Err(err) => Err(err),
        }
    }

    fn read(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<SecretValue, SecretStoreError> {
        let mut search = self.search(reference);
        search.load_data(true);
        let results = self.call(interaction, || search.search())?;
        let bytes = results
            .into_iter()
            .find_map(|result| match result {
                SearchResult::Data(bytes) => Some(bytes),
                _ => None,
            })
            .ok_or(SecretStoreError::NotFound)?;
        SecretValue::from_utf8(bytes).map_err(|_| SecretStoreError::Backend { code: ERR_DECODE })
    }

    fn write(
        &self,
        reference: &SecretRef,
        value: &SecretValue,
        mode: WriteMode,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        match mode {
            WriteMode::Create => self.add(reference, value, interaction),
            WriteMode::Replace => self.update(reference, value, interaction),
            WriteMode::Upsert => match self.add(reference, value, interaction) {
                Err(SecretStoreError::AlreadyExists) => self.update(reference, value, interaction),
                other => other,
            },
        }
    }

    fn delete(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        let search = self.search(reference);
        self.call(interaction, || search.delete())
    }
}

/// The lock state of `keychain` right now, or `Unknown` when it cannot be
/// read.
fn observed_state(keychain: &SecKeychain) -> StoreState {
    match keychain_unlocked(keychain) {
        Ok(true) => StoreState::Available,
        Ok(false) => StoreState::Locked,
        Err(_) => StoreState::Unknown,
    }
}

fn keychain_path(target: &KeychainTarget) -> PathBuf {
    match target {
        KeychainTarget::Login => PathBuf::from(LOGIN_KEYCHAIN),
        KeychainTarget::File { path, .. } => path.clone(),
    }
}

/// Opens or creates the file keychain at `path`, keeping its password in
/// `password_file` in the same private folder, and unlocks it.
fn open_file_keychain(
    path: &Path,
    password_file: &Path,
) -> Result<SecKeychain, KeychainSetupError> {
    let folder = match (path.parent(), password_file.parent()) {
        (Some(folder), Some(other)) if folder == other => folder,
        _ => {
            return Err(KeychainSetupError::SplitFolders {
                path: path.to_path_buf(),
                password_file: password_file.to_path_buf(),
            });
        }
    };
    ensure_private_folder(folder)?;
    let fs = MacosFilesystem;
    let keychain_error = |step: &'static str| {
        move |err: security_framework::base::Error| KeychainSetupError::Keychain {
            step,
            path: path.to_path_buf(),
            code: err.code(),
        }
    };
    let (mut keychain, password) = match fs.symlink_metadata(path)? {
        Some(info) if info.kind == FileKind::File => {
            let password = read_password(&fs, path, password_file)?;
            let keychain = SecKeychain::open(path).map_err(keychain_error("open"))?;
            (keychain, password)
        }
        Some(_) => {
            return Err(PlatformError::NotARegularFile {
                path: path.to_path_buf(),
            }
            .into());
        }
        None => {
            let password = generate_password(path)?;
            fs.write_file_atomic(password_file, password.expose().as_bytes())?;
            let keychain = CreateOptions::new()
                .password(password.expose())
                .prompt_user(false)
                .create(path)
                .map_err(keychain_error("create"))?;
            (keychain, password)
        }
    };
    keychain
        .unlock(Some(password.expose()))
        .map_err(keychain_error("unlock"))?;
    let mut settings = KeychainSettings::new();
    settings.set_lock_on_sleep(false);
    settings.set_lock_interval(None);
    keychain
        .set_settings(&settings)
        .map_err(keychain_error("configure"))?;
    Ok(keychain)
}

/// Creates `folder` with mode 0700 when it is missing (never its parent),
/// then checks that it is a private folder of this user.
fn ensure_private_folder(folder: &Path) -> Result<(), KeychainSetupError> {
    let fs = MacosFilesystem;
    if fs.symlink_metadata(folder)?.is_none() {
        match fs.create_dir_private(folder) {
            Ok(()) | Err(PlatformError::AlreadyExists { .. }) => {}
            Err(err) => return Err(err.into()),
        }
    }
    let info = fs
        .symlink_metadata(folder)?
        .ok_or_else(|| PlatformError::NotFound {
            path: folder.to_path_buf(),
        })?;
    let user = MacosIdentity.current_user()?;
    check_private_dir(&info, &user).map_err(|violation| KeychainSetupError::NotPrivate {
        path: folder.to_path_buf(),
        violation,
    })
}

fn read_password(
    fs: &MacosFilesystem,
    path: &Path,
    password_file: &Path,
) -> Result<SecretValue, KeychainSetupError> {
    let invalid = || KeychainSetupError::PasswordFile {
        path: path.to_path_buf(),
        password_file: password_file.to_path_buf(),
    };
    let bytes = match fs.read_file_limited(password_file, FILE_PASSWORD_MAX) {
        Ok(bytes) => bytes,
        Err(PlatformError::NotFound { .. } | PlatformError::FileTooLarge { .. }) => {
            return Err(invalid());
        }
        Err(err) => return Err(err.into()),
    };
    SecretValue::from_utf8(bytes).map_err(|_| invalid())
}

/// Returns a random password of [`FILE_PASSWORD_LEN`] characters from
/// [`PASSWORD_ALPHABET`], from `SecRandomCopyBytes`.
fn generate_password(path: &Path) -> Result<SecretValue, KeychainSetupError> {
    // The largest multiple of the alphabet size below 256, so every
    // character is equally likely.
    const ACCEPT_BELOW: u8 = 248;
    let random = SecRandom::default();
    let mut password = String::with_capacity(FILE_PASSWORD_LEN);
    let mut buf = [0_u8; 64];
    while password.len() < FILE_PASSWORD_LEN {
        random
            .copy_bytes(&mut buf)
            .map_err(|err| KeychainSetupError::Keychain {
                step: "generate a password for",
                path: path.to_path_buf(),
                code: err.raw_os_error().unwrap_or(-1),
            })?;
        for byte in buf.iter().copied().filter(|byte| *byte < ACCEPT_BELOW) {
            if password.len() == FILE_PASSWORD_LEN {
                break;
            }
            let index = usize::from(byte) % PASSWORD_ALPHABET.len();
            password.push(char::from(PASSWORD_ALPHABET[index]));
        }
    }
    buf.fill(0);
    SecretValue::new(password).map_err(|_| KeychainSetupError::Keychain {
        step: "generate a password for",
        path: path.to_path_buf(),
        code: -1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEVER: Interaction = Interaction::Never;
    const PROMPT: Interaction = Interaction::AllowPrompt;

    #[test]
    fn map_status_follows_the_spec_table() {
        use SecretStoreError::{AccessDenied, AlreadyExists, Backend, Locked, NotFound};

        // The literal OSStatus values, so the table also pins the constants.
        let cases = [
            (-25_300, false, NEVER, NotFound),
            (-25_300, true, NEVER, NotFound),
            (-25_300, true, PROMPT, NotFound),
            (-25_299, false, NEVER, AlreadyExists),
            (-25_299, true, PROMPT, AlreadyExists),
            (-25_293, true, NEVER, Locked),
            (-25_293, true, PROMPT, Locked),
            (-25_293, false, NEVER, AccessDenied { by_user: false }),
            (-25_293, false, PROMPT, AccessDenied { by_user: true }),
            (-25_308, true, NEVER, Locked),
            (-25_308, false, NEVER, AccessDenied { by_user: false }),
            (-25_308, false, PROMPT, AccessDenied { by_user: false }),
            (-128, false, PROMPT, AccessDenied { by_user: true }),
            (-128, true, PROMPT, AccessDenied { by_user: true }),
            (-26_275, false, NEVER, Backend { code: -26_275 }),
            (-25_291, false, NEVER, Backend { code: -25_291 }),
            (-50, true, PROMPT, Backend { code: -50 }),
        ];
        for (code, locked, interaction, expected) in cases {
            assert_eq!(
                map_status(code, locked, interaction),
                expected,
                "{code} locked={locked} {interaction}"
            );
        }
    }

    #[test]
    fn a_file_target_keys_by_its_folder_and_file_name() {
        let target = KeychainTarget::in_dir(Path::new("/root/data/keychain"));

        assert_eq!(
            target,
            KeychainTarget::File {
                path: PathBuf::from("/root/data/keychain/stackly.keychain-db"),
                password_file: PathBuf::from("/root/data/keychain/stackly.keychain-password"),
            }
        );
        assert_eq!(target.key_text(), "file:keychain/stackly.keychain-db");
        assert_eq!(KeychainTarget::Login.key_text(), "login");
    }

    #[test]
    fn generated_passwords_have_32_alphanumeric_characters_and_differ() {
        let path = Path::new("/unused");
        let first = generate_password(path).unwrap();
        let second = generate_password(path).unwrap();

        for password in [&first, &second] {
            assert_eq!(password.len(), FILE_PASSWORD_LEN);
            assert!(password.expose().bytes().all(|b| b.is_ascii_alphanumeric()));
        }
        assert_ne!(first.expose(), second.expose());
    }
}
