//! The real `stacklyd` binary and its secret store (spec 0011 build step
//! 5): with `STACKLY_ROOT` set, the daemon opens a file keychain of its own
//! under the data root, never the login Keychain, and serves `secret.*`
//! over it. The test uses only that file keychain and a fixture credential.
#![cfg(target_os = "macos")]

mod common;

use common::{client, daemon_paths, runtime, start_daemon};
use stackly_protocol::secret::{
    RevealPurpose, RevealSecretParams, SecretInput, SecretMode, SecretStatus, SecretStoreBackend,
    SecretStoreTarget, SecretText, SetSecretParams,
};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::FixtureCredential;

const REFERENCE: &str = "local:erp/postgres/admin";

#[test]
fn a_daemon_with_a_root_override_serves_secrets_from_a_file_keychain_under_the_data_root() {
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let paths = daemon_paths(&root);
    let credential = FixtureCredential::new();
    let rt = runtime();

    let (info, stored, revealed) = rt.block_on(async {
        let (client, _) = client(daemon.socket()).await;
        let Reply::DaemonGetInfo(info) = client.call(Call::DaemonGetInfo).await.unwrap() else {
            panic!("not an info reply");
        };
        let Reply::SecretSet(stored) = client
            .call(Call::SecretSet(SetSecretParams {
                reference: REFERENCE.to_owned(),
                input: SecretInput::Value(SecretText::new(credential.value().to_owned())),
                mode: SecretMode::Create,
                expected_revision: None,
                confirm_stored_only: false,
            }))
            .await
            .unwrap()
        else {
            panic!("not a set reply");
        };
        let Reply::SecretReveal(revealed) = client
            .call(Call::SecretReveal(RevealSecretParams {
                reference: REFERENCE.to_owned(),
                purpose: RevealPurpose::Show,
            }))
            .await
            .unwrap()
        else {
            panic!("not a reveal reply");
        };
        (info, stored, revealed)
    });

    let store = info.secret_store.expect("the store is reported");
    assert_eq!(store.backend, SecretStoreBackend::MacosKeychain);
    assert_eq!(store.target, Some(SecretStoreTarget::File));
    assert_eq!(stored.summary.status, SecretStatus::Present);
    assert_eq!(revealed.value.expose(), credential.value());
    let keychain_dir = paths.layout.keychain_dir();
    assert!(
        keychain_dir.join("stackly.keychain-db").exists(),
        "the file keychain is under {}",
        keychain_dir.display()
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    credential.assert_absent(
        &std::fs::read_to_string(daemon_log(&paths)).unwrap_or_default(),
        "the daemon log",
    );
}

fn daemon_log(paths: &common::DaemonPaths) -> std::path::PathBuf {
    paths.layout.daemon_log_dir().join("stacklyd.log")
}
