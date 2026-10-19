//! `MacosSecretStore` against a real file keychain under a `TestRoot`
//! (spec 0011, AC-2 and AC-3): create, probe, read, replace, upsert, and
//! delete; the user's keychain search list stays the same; and a locked
//! keychain and an item whose ACL does not trust this process answer
//! without a dialog in under a second. The login Keychain is only ever
//! searched by attributes, never written. The Security framework needs an
//! active GUI login session; without one the evidence is `BLOCKED`.
#![cfg(target_os = "macos")]

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use stackly_domain::SecretRef;
use stackly_platform::{
    Interaction, PrivateDirViolation, SecretBackend, SecretPresence, SecretStore, SecretStoreError,
    SecretValue, StoreState, WriteMode,
};
use stackly_platform_macos::{KeychainSetupError, KeychainTarget, MacosSecretStore};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Evidence, FixtureCredential, Kind, evidence};

/// The longest a call that may never show a dialog is allowed to take.
const NO_DIALOG_LIMIT: Duration = Duration::from_secs(1);
/// Who can unblock a run without a GUI login session.
const GUI_OWNER: &str = "the user of this Mac, logged in to a GUI session";

/// Runs a command and returns its output.
fn run(program: &str, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .output()
        .expect("the macOS command runs")
}

/// The user's keychain search list, as `security` prints it.
fn search_list() -> String {
    let output = run("security", &["list-keychains", "-d", "user"]);
    assert!(output.status.success(), "security list-keychains failed");
    String::from_utf8(output.stdout).expect("security prints UTF-8")
}

/// Notes the run as blocked and returns false when this process has no
/// GUI login session, which the Security framework needs.
fn has_gui_session(ev: &Evidence) -> bool {
    let output = run("launchctl", &["managername"]);
    let manager = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if manager == "Aqua" {
        return true;
    }
    ev.blocked(
        &format!("no GUI login session (launchd manager `{manager}`)"),
        GUI_OWNER,
    );
    false
}

/// A test root with a private data folder, and the file keychain target
/// of its `keychain/` folder.
struct Keychain {
    _root: TestRoot,
    dir: PathBuf,
    target: KeychainTarget,
}

impl Keychain {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root");
        let data = root.roots().data().to_path_buf();
        fs::create_dir(&data).expect("the test root accepts a data folder");
        fs::set_permissions(&data, Permissions::from_mode(0o700))
            .expect("the test owns its data folder");
        let dir = data.join("keychain");
        let target = KeychainTarget::in_dir(&dir);
        Self {
            _root: root,
            dir,
            target,
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("stackly.keychain-db")
    }

    fn password_file(&self) -> PathBuf {
        self.dir.join("stackly.keychain-password")
    }

    fn open(&self) -> MacosSecretStore {
        MacosSecretStore::new(self.target.clone()).expect("the file keychain opens")
    }

    fn path_text(&self) -> String {
        self.path().to_str().expect("a UTF-8 path").to_owned()
    }
}

/// A reference unique to this run, so the search of the user's own
/// keychains can never find an item from another test.
fn unique_reference(name: &str) -> SecretRef {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_nanos();
    SecretRef::parse(&format!("local:test/{name}-{}-{nanos}", std::process::id()))
        .expect("a valid test reference")
}

fn value(credential: &FixtureCredential) -> SecretValue {
    SecretValue::new(credential.value().to_owned()).expect("a fixture credential is a valid value")
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .expect("the path exists")
        .permissions()
        .mode()
        & 0o7777
}

/// Runs `call`, checks it took less than [`NO_DIALOG_LIMIT`], and returns
/// its result with the time it took.
fn timed<T>(what: &str, call: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let result = call();
    let took = started.elapsed();
    assert!(took < NO_DIALOG_LIMIT, "{what} took {took:?}, a dialog?");
    (result, took)
}

#[test]
fn a_file_keychain_stores_secrets_apart_from_the_users_search_list() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "MacosSecretStore on a file keychain under the test root creates, probes, reads, replaces, upserts, and deletes generic passwords with service stackly-secret, reopens the keychain with its password file, keeps the folder 0700 and the password file 0600, and leaves the user's keychain search list unchanged with the items invisible from it",
    );
    if !has_gui_session(&ev) {
        return;
    }
    let before = search_list();
    let keychain = Keychain::new();
    let store = keychain.open();
    let (first, second, third) = (
        FixtureCredential::new(),
        FixtureCredential::new(),
        FixtureCredential::new(),
    );
    let admin = unique_reference("admin");
    let other = unique_reference("other");

    assert_eq!(store.backend(), SecretBackend::MacosKeychain);
    assert_eq!(store.state(), StoreState::Available);
    assert_eq!(mode(&keychain.dir), 0o700);
    assert_eq!(mode(&keychain.password_file()), 0o600);
    assert_eq!(
        store.store_key(&admin),
        format!(
            "macos-keychain:file:keychain/stackly.keychain-db/stackly-secret/{}",
            admin.path()
        )
    );

    assert_eq!(store.probe(&admin), Ok(SecretPresence::Missing));
    let missing = store.read(&admin, Interaction::Never).unwrap_err();
    assert_eq!(missing, SecretStoreError::NotFound);
    let replace_missing = store.write(
        &admin,
        &value(&first),
        WriteMode::Replace,
        Interaction::Never,
    );
    assert_eq!(replace_missing, Err(SecretStoreError::NotFound));

    store
        .write(
            &admin,
            &value(&first),
            WriteMode::Create,
            Interaction::Never,
        )
        .unwrap();
    assert_eq!(store.probe(&admin), Ok(SecretPresence::Present));
    let read = store.read(&admin, Interaction::Never).unwrap();
    assert!(
        read.expose() == first.value(),
        "read gives the created value"
    );
    let duplicate = store.write(
        &admin,
        &value(&second),
        WriteMode::Create,
        Interaction::Never,
    );
    assert_eq!(duplicate, Err(SecretStoreError::AlreadyExists));

    store
        .write(
            &admin,
            &value(&second),
            WriteMode::Replace,
            Interaction::Never,
        )
        .unwrap();
    let read = store.read(&admin, Interaction::Never).unwrap();
    assert!(read.expose() == second.value(), "replace changes the value");
    store
        .write(
            &admin,
            &value(&third),
            WriteMode::Upsert,
            Interaction::Never,
        )
        .unwrap();
    store
        .write(
            &other,
            &value(&first),
            WriteMode::Upsert,
            Interaction::Never,
        )
        .unwrap();
    let read = store.read(&admin, Interaction::Never).unwrap();
    assert!(read.expose() == third.value(), "upsert replaces a value");

    let account = admin.path().to_owned();
    let in_file = run(
        "security",
        &[
            "find-generic-password",
            "-s",
            "stackly-secret",
            "-a",
            &account,
            &keychain.path_text(),
        ],
    );
    assert!(in_file.status.success(), "the item is in the file keychain");
    let in_search_list = run(
        "security",
        &[
            "find-generic-password",
            "-s",
            "stackly-secret",
            "-a",
            &account,
        ],
    );
    assert!(
        !in_search_list.status.success(),
        "the item is not visible from the user's search list"
    );

    store.delete(&admin, Interaction::Never).unwrap();
    assert_eq!(store.probe(&admin), Ok(SecretPresence::Missing));
    assert_eq!(
        store.delete(&admin, Interaction::Never),
        Err(SecretStoreError::NotFound)
    );

    drop(store);
    let reopened = keychain.open();
    let read = reopened.read(&other, Interaction::Never).unwrap();
    assert!(
        read.expose() == first.value(),
        "a reopened keychain keeps its items"
    );

    assert_eq!(search_list(), before, "the user's search list is unchanged");
    ev.pass_with("all six store calls behaved per the write modes; the search list and the login keychain did not see the items");
}

#[test]
fn a_locked_file_keychain_refuses_reads_and_writes_without_a_dialog() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "on a file keychain locked with security lock-keychain and interaction Never, read and write fail Locked, probe still answers Present or Missing, a missing item stays NotFound, delete succeeds, state() is locked, and every call ends in under a second without a dialog",
    );
    if !has_gui_session(&ev) {
        return;
    }
    let keychain = Keychain::new();
    let store = keychain.open();
    let credential = FixtureCredential::new();
    let kept = unique_reference("kept");
    let missing = unique_reference("missing");
    store
        .write(
            &kept,
            &value(&credential),
            WriteMode::Create,
            Interaction::Never,
        )
        .unwrap();
    let lock = run("security", &["lock-keychain", &keychain.path_text()]);
    assert!(lock.status.success(), "security lock-keychain failed");

    let (read, read_took) = timed("read", || store.read(&kept, Interaction::Never));
    assert_eq!(read.unwrap_err(), SecretStoreError::Locked);
    let (write, write_took) = timed("write", || {
        store.write(
            &kept,
            &value(&credential),
            WriteMode::Upsert,
            Interaction::Never,
        )
    });
    assert_eq!(write, Err(SecretStoreError::Locked));
    assert_eq!(store.state(), StoreState::Locked);
    let (probe, _) = timed("probe", || store.probe(&kept));
    assert_eq!(probe, Ok(SecretPresence::Present));
    let (probe_missing, _) = timed("probe", || store.probe(&missing));
    assert_eq!(probe_missing, Ok(SecretPresence::Missing));
    let (read_missing, _) = timed("read", || store.read(&missing, Interaction::Never));
    assert_eq!(read_missing.unwrap_err(), SecretStoreError::NotFound);
    let (delete, _) = timed("delete", || store.delete(&kept, Interaction::Never));
    assert_eq!(delete, Ok(()));
    assert_eq!(store.state(), StoreState::Locked);

    ev.pass_with(&format!(
        "read Locked in {read_took:?}, write Locked in {write_took:?}, probe Present and Missing, missing read NotFound, delete Ok, state locked"
    ));
}

#[test]
fn an_item_that_does_not_trust_this_process_is_denied_without_a_dialog() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "an item made with security add-generic-password -T /usr/bin/true in the file keychain reads as AccessDenied with by_user false under interaction Never, in under a second without a dialog, while probe still answers Present",
    );
    if !has_gui_session(&ev) {
        return;
    }
    let keychain = Keychain::new();
    let store = keychain.open();
    let credential = FixtureCredential::new();
    let foreign = unique_reference("foreign");
    let add = run(
        "security",
        &[
            "add-generic-password",
            "-s",
            "stackly-secret",
            "-a",
            foreign.path(),
            "-w",
            credential.value(),
            "-T",
            "/usr/bin/true",
            &keychain.path_text(),
        ],
    );
    assert!(add.status.success(), "security add-generic-password failed");

    let (read, took) = timed("read", || store.read(&foreign, Interaction::Never));
    assert_eq!(
        read.unwrap_err(),
        SecretStoreError::AccessDenied { by_user: false }
    );
    assert_eq!(store.probe(&foreign), Ok(SecretPresence::Present));
    assert_eq!(store.state(), StoreState::Available);

    ev.pass_with(&format!(
        "read AccessDenied {{ by_user: false }} in {took:?}, probe Present"
    ));
}

#[test]
fn a_keychain_folder_open_to_others_is_rejected_and_left_alone() {
    let keychain = Keychain::new();
    fs::create_dir(&keychain.dir).unwrap();
    fs::set_permissions(&keychain.dir, Permissions::from_mode(0o755)).unwrap();

    let err = MacosSecretStore::new(keychain.target.clone()).unwrap_err();

    assert_eq!(
        err,
        KeychainSetupError::NotPrivate {
            path: keychain.dir.clone(),
            violation: PrivateDirViolation::OpenToOthers,
        }
    );
    assert_eq!(mode(&keychain.dir), 0o755);
    assert!(!keychain.path().exists());
    assert!(!keychain.password_file().exists());
}

#[test]
fn an_existing_keychain_without_its_password_file_is_rejected() {
    let ev = evidence!(
        covers = ["REQ-016"],
        kind = Kind::Fixture,
        expected = "a file keychain whose password file was removed is refused at setup with KeychainSetupError::PasswordFile, never opened locked",
    );
    if !has_gui_session(&ev) {
        return;
    }
    let keychain = Keychain::new();
    drop(keychain.open());
    fs::remove_file(keychain.password_file()).unwrap();

    let err = MacosSecretStore::new(keychain.target.clone()).unwrap_err();

    assert_eq!(
        err,
        KeychainSetupError::PasswordFile {
            path: keychain.path(),
            password_file: keychain.password_file(),
        }
    );
    ev.pass();
}
