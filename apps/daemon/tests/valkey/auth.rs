//! Bind and auth of a Valkey instance (spec 0040 AC-4): one loopback
//! listener, `NOAUTH` without a sign in, the `default` user with the
//! Keychain password in both `AUTH` forms, `WRONGPASS` for another value, a
//! password with a space and quotes, a readiness user that may only `PING`,
//! and a password that never reaches argv, a file, a log, or the registry,
//! while `users.conf` holds only its hash and goes away after a clean stop.
//! The scans cover the test root and the runtime root, where the run folder
//! with the spools and `secrets/` lives, in the raw form of the password and
//! in its JSON escaped form, the one a JSON log or registry text would hold.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    READINESS_USER, Reply, Site, Valkey, create_with, output_of, pid_of, port_of, sockets,
    stop_instance,
};

/// The password with a space, both quotes, and a backslash.
const ODD_PASSWORD: &str = "p w\"x\\y'z";

/// The forms of [`ODD_PASSWORD`] a leak could take: as it is, and JSON
/// escaped (`p w\"x\\y'z`), the form a JSON log line, a JSON registry
/// column, or a `valkeyConf` value would hold.
fn password_forms() -> [String; 2] {
    let json = serde_json::to_string(ODD_PASSWORD).expect("a string serializes");
    [ODD_PASSWORD.to_owned(), json.trim_matches('"').to_owned()]
}

/// The SHA-256 of [`ODD_PASSWORD`] as lowercase hex.
fn password_hash() -> String {
    Sha256::digest(ODD_PASSWORD.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The folders a leak is searched in: the test root (registry, data,
/// config, config history, logs, runtime install) and the runtime root of
/// the daemon, which holds the run folder with `stdout.spool`,
/// `stderr.spool`, and `secrets/`.
fn scanned_roots(site: &Site) -> [PathBuf; 2] {
    let runtime = site
        .platform
        .directories()
        .roots()
        .expect("the macOS platform resolved its roots")
        .runtime()
        .to_path_buf();
    [site.root.path().to_path_buf(), runtime]
}

/// Every file of [`scanned_roots`], except the Keychain folder, that holds
/// one of the [`password_forms`].
fn password_files(site: &Site) -> Vec<String> {
    let keychain = site.prepared.layout().keychain_dir();
    let mut found = Vec::new();
    for root in scanned_roots(site) {
        for form in password_forms() {
            found.extend(files_holding(&root, &[&keychain], form.as_bytes()));
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Every file of [`scanned_roots`] that holds the hash of the password.
fn hash_files(site: &Site) -> Vec<String> {
    let hash = password_hash();
    let mut found = Vec::new();
    for root in scanned_roots(site) {
        found.extend(files_holding(&root, &[], hash.as_bytes()));
    }
    found.sort();
    found.dedup();
    found
}

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

/// Commands the readiness user must not run: reads and writes of keys,
/// server and config introspection, ACL, client and pub/sub commands,
/// scripts, snapshots, and shutdown.
const READINESS_REFUSED: &[&[&str]] = &[
    &["GET", "k"],
    &["SET", "k", "v"],
    &["DEL", "k"],
    &["EXISTS", "k"],
    &["KEYS", "*"],
    &["SCAN", "0"],
    &["DBSIZE"],
    &["INFO"],
    &["INFO", "server"],
    &["CONFIG", "GET", "port"],
    &["CONFIG", "SET", "maxmemory", "1mb"],
    &["CONFIG", "REWRITE"],
    &["ACL", "LIST"],
    &["ACL", "SETUSER", "x", "on", "nopass", "+@all"],
    &["ACL", "DELUSER", "default"],
    &["CLIENT", "LIST"],
    &["CLIENT", "KILL", "TYPE", "normal"],
    &["PUBLISH", "c", "m"],
    &["SUBSCRIBE", "c"],
    &["EVAL", "return 1", "0"],
    &["FLUSHALL"],
    &["SAVE"],
    &["BGSAVE"],
    &["SHUTDOWN", "NOSAVE"],
];

fn is_error(reply: &Reply, prefix: &str) -> bool {
    reply.error().is_some_and(|text| text.starts_with(prefix))
}

// covers: AC-4 (spec 0040)
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
    for command in READINESS_REFUSED {
        let reply = readiness.cmd(command);
        assert!(is_error(&reply, "NOPERM"), "{command:?}: {reply:?}");
    }
    // `enable-debug-command no`, the Valkey default, refuses DEBUG before
    // the ACL check, for every user.
    let debug = readiness.cmd(&["DEBUG", "SLEEP", "0"]);
    assert!(
        debug
            .error()
            .is_some_and(|text| text.contains("DEBUG command not allowed")),
        "{debug:?}"
    );
    assert_eq!(
        readiness.cmd(&["PING"]),
        Reply::Simple("PONG".to_owned()),
        "the server is still up after the refused commands"
    );

    let users = site.users_conf(made.id);
    let mode = std::fs::metadata(&users)
        .expect("users.conf exists while the instance runs")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "users.conf is private");
    let text = std::fs::read_to_string(&users).expect("users.conf reads");
    let hash = password_hash();
    assert!(text.contains(&format!("#{hash}")), "{text}");
    assert_eq!(
        hash_files(&site),
        [users.display().to_string()],
        "the hash is only in secrets/users.conf"
    );
    let run = site.instance_paths(made.id).run;
    for spool in ["stdout.spool", "stderr.spool"] {
        assert!(
            run.join(spool).exists(),
            "{spool} is in the scanned run folder {}",
            run.display()
        );
    }

    let processes = output_of("/bin/ps", &["-axww", "-o", "args="]);
    let environment = output_of(
        "/bin/ps",
        &["-E", "-ww", "-o", "command=", "-p", &pid.to_string()],
    );
    assert!(
        environment.contains("TZ=UTC"),
        "ps -E shows the environment of valkey-server: {environment}"
    );
    for form in password_forms() {
        assert!(!processes.contains(&form), "the password is in an argv");
        assert!(
            !environment.contains(&form),
            "the password is in the environment of valkey-server"
        );
        assert!(
            !daemon.stderr().contains(&form),
            "the password is in the daemon log"
        );
    }
    let holding = password_files(&site);
    assert!(holding.is_empty(), "the password is in {holding:?}");

    stop_instance(&api, made.id).await;
    assert!(!users.exists(), "users.conf is removed after a clean stop");
    let holding = password_files(&site);
    assert!(holding.is_empty(), "the password is in {holding:?}");
    let hashes = hash_files(&site);
    assert!(
        hashes.is_empty(),
        "the hash outlives the stop in {hashes:?}"
    );
    for form in password_forms() {
        assert!(
            !daemon.stderr().contains(&form),
            "the password is in the daemon log"
        );
    }
    daemon.stop();
    ev.pass_with(&format!(
        "Valkey {version}: one listener 127.0.0.1:{port}, NOAUTH, WRONGPASS, NOPERM as expected, users.conf held only the hash and was removed"
    ));
}

/// A characterization of Valkey 9.1.2, not a promise of Stackly: commands
/// flagged `no-auth` (`HELLO`, `AUTH`) skip the ACL command check, so the
/// readiness user, whose rules are `-@all +ping`, still gets the `HELLO`
/// reply with the server name, version, and role. The spec records that as
/// a residual in the same class as the `PONG` itself; this test fails when
/// a Valkey release starts to check `HELLO`, so the residual can go.
// covers: AC-4 (spec 0040, the readiness user residual)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_answers_hello_to_the_readiness_user_without_an_acl_check() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "signed in as stackly-readiness, HELLO without arguments answers the RESP2 server map with server valkey and the version, because Valkey skips the ACL check for no-auth commands; GET is still NOPERM and PING is PONG",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = crate::harness::create_instance(&site, &api, "hello", json!({})).await;
    let running = api.start_ready(made.id).await;
    let port = port_of(&running);
    let version = Valkey::signed_in(port, &made.password).info_server("valkey_version");
    site.subject(&ev, &version);

    let mut readiness = Valkey::connect(port);
    assert_eq!(
        readiness.cmd(&["AUTH", READINESS_USER, "anything"]),
        Reply::Simple("OK".to_owned())
    );
    let hello = readiness.cmd(&["HELLO"]);
    let Reply::Array(Some(items)) = &hello else {
        panic!("HELLO answers a map as a RESP2 array: {hello:?}");
    };
    let texts: Vec<&str> = items.iter().filter_map(Reply::text).collect();
    let field = |name: &str| {
        texts
            .iter()
            .position(|text| *text == name)
            .and_then(|at| texts.get(at + 1))
            .copied()
    };
    assert_eq!(field("server"), Some("valkey"), "{hello:?}");
    assert_eq!(field("version"), Some(version.as_str()), "{hello:?}");
    assert!(is_error(&readiness.cmd(&["GET", "k"]), "NOPERM"));
    assert_eq!(readiness.cmd(&["PING"]), Reply::Simple("PONG".to_owned()));
    crate::harness::stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "Valkey {version}: HELLO answered the readiness user with server valkey and version {version}; GET was NOPERM"
    ));
}
