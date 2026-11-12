//! Bind and auth of a Valkey instance (spec 0040 AC-4): one loopback
//! listener, `NOAUTH` without a sign in, the `default` user with the
//! Keychain password in both `AUTH` forms, `WRONGPASS` for another value, a
//! password with a space and quotes, a readiness user that may only `PING`,
//! and a password that never reaches argv, a file, a log, or the registry,
//! while `users.conf` holds only its hash and goes away after a clean stop.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    READINESS_USER, Reply, Site, Valkey, create_with, output_of, pid_of, port_of, sockets,
    stop_instance,
};

/// The password with a space, both quotes, and a backslash.
const ODD_PASSWORD: &str = "p w\"x\\y'z";

/// Every file under `dir`, except the folders in `skip`, that holds `needle`.
fn files_holding(dir: &Path, skip: &[&Path], needle: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        if skip.iter().any(|skipped| folder.starts_with(skipped)) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file()
                && std::fs::read(&path)
                    .is_ok_and(|bytes| bytes.windows(needle.len()).any(|part| part == needle))
            {
                found.push(path.display().to_string());
            }
        }
    }
    found
}

fn is_error(reply: &Reply, prefix: &str) -> bool {
    reply.error().is_some_and(|text| text.starts_with(prefix))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_listens_on_loopback_only_and_signs_in_only_with_its_password() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "valkey-server listens only on its loopback client port with no Unix socket; a command without AUTH gets NOAUTH; AUTH <password> and AUTH default <password> sign in, another value gets WRONGPASS, a password with a space, quotes, and a backslash signs in; stackly-readiness may PING and gets NOPERM for GET, INFO, and CONFIG GET; the password is in no argv, no file of the root, no daemon log, and not in the registry; users.conf is 0600 with only the SHA-256 of the password and is gone after a clean stop",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.binary(&[]);
    let api = site.binary_api(&daemon).await;
    let made = create_with(&site, &api, "auth", Some(ODD_PASSWORD), json!({})).await;
    let running = api.start_ready(made.id).await;
    let port = port_of(&running);
    let pid = pid_of(&running);

    let (listening, udp, unix) = sockets(pid);
    assert_eq!(listening, [format!("127.0.0.1:{port}")], "one TCP listener");
    assert_eq!(udp, 0, "no UDP socket");
    assert_eq!(unix, 0, "no Unix socket");

    let mut anonymous = Valkey::connect(port);
    assert!(is_error(&anonymous.cmd(&["GET", "k"]), "NOAUTH"));
    assert!(is_error(
        &anonymous.cmd(&["AUTH", "not-the-password"]),
        "WRONGPASS"
    ));
    assert!(is_error(
        &anonymous.cmd(&["AUTH", "default", "not-the-password"]),
        "WRONGPASS"
    ));
    assert_eq!(
        anonymous.cmd(&["AUTH", ODD_PASSWORD]),
        Reply::Simple("OK".to_owned())
    );
    let mut named = Valkey::connect(port);
    assert_eq!(
        named.cmd(&["AUTH", "default", ODD_PASSWORD]),
        Reply::Simple("OK".to_owned())
    );
    let version = named.info_server("valkey_version");
    site.subject(&ev, &version);

    let mut readiness = Valkey::connect(port);
    assert_eq!(
        readiness.cmd(&["AUTH", READINESS_USER, "anything"]),
        Reply::Simple("OK".to_owned()),
        "the readiness user has nopass"
    );
    assert_eq!(readiness.cmd(&["PING"]), Reply::Simple("PONG".to_owned()));
    for command in [&["GET", "k"][..], &["INFO"], &["CONFIG", "GET", "port"]] {
        let reply = readiness.cmd(command);
        assert!(is_error(&reply, "NOPERM"), "{command:?}: {reply:?}");
    }

    let users = site.users_conf(made.id);
    let mode = std::fs::metadata(&users)
        .expect("users.conf exists while the instance runs")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "users.conf is private");
    let text = std::fs::read_to_string(&users).expect("users.conf reads");
    let hash: String = Sha256::digest(ODD_PASSWORD.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert!(text.contains(&format!("#{hash}")), "{text}");

    let processes = output_of("/bin/ps", &["-axww", "-o", "args="]);
    assert!(
        !processes.contains(ODD_PASSWORD),
        "the password is in an argv"
    );
    let keychain = site.prepared.layout().keychain_dir();
    let holding = files_holding(site.root.path(), &[&keychain], ODD_PASSWORD.as_bytes());
    assert!(holding.is_empty(), "the password is in {holding:?}");
    assert!(
        !daemon.stderr().contains(ODD_PASSWORD),
        "the password is in the daemon log"
    );

    stop_instance(&api, made.id).await;
    assert!(!users.exists(), "users.conf is removed after a clean stop");
    let holding = files_holding(site.root.path(), &[&keychain], ODD_PASSWORD.as_bytes());
    assert!(holding.is_empty(), "the password is in {holding:?}");
    daemon.stop();
    ev.pass_with(&format!(
        "Valkey {version}: one listener 127.0.0.1:{port}, NOAUTH, WRONGPASS, NOPERM as expected, users.conf held only the hash and was removed"
    ));
}
