//! What a ClickHouse instance opens and who gets in (spec 0043 AC-4,
//! AC-6): exactly two loopback TCP listeners and no UDP socket, every path
//! in `system.server_settings` inside the instance, no file outside the
//! instance folders after start, stop, and start, the `default` user only
//! with the Keychain password over HTTP and native TCP, the password never
//! on disk, in the log, or in argv, and the two managed users that can do
//! nothing but their one job.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    READINESS_USER, RELOAD_USER, Site, alive, assert_two_listeners, create_instance, entries_under,
    eventually, http_query, log_lines, output_of, pid_of, port_of, stop_and_shutdown,
    stop_instance,
};

/// The four `system.server_settings` values that start with `/` but are
/// paths inside Keeper, not files, and unused without Keeper (spec 0043,
/// *Path*).
const KEEPER_PATHS: [&str; 4] = [
    "default_replica_path",
    "series_keeper_path",
    "distributed_ddl.path",
    "distributed_ddl.replicas_path",
];

/// The SHA-256 of `text` as 64 lowercase hex characters.
pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Every file under `folder` whose bytes hold `needle`.
pub fn files_holding(folder: &Path, needle: &[u8]) -> Vec<PathBuf> {
    entries_under(folder)
        .into_iter()
        .map(|relative| folder.join(relative))
        .filter(|path| path.is_file())
        .filter(|path| {
            std::fs::read(path)
                .is_ok_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
        })
        .collect()
}

// covers: AC-4 (spec 0043)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn clickhouse_only_listens_on_its_two_registry_ports() {
    let ev = evidence!(
        covers = ["REQ-037", "AC-037"],
        kind = Kind::Native,
        expected = "a running ClickHouse instance holds exactly two TCP LISTEN sockets on 127.0.0.1 at its http and native registry ports and no UDP socket, every path in system.server_settings is inside the instance or one of the four Keeper paths, and after start, stop, and start no file outside the instance's data, config, temp, and run folders and the log store was added and no .clickhouse file is in the process HOME",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "listeners", json!({})).await;
    let paths = site.instance_paths(made.id);
    let logs_root = site
        .platform
        .directories()
        .roots()
        .expect("the macOS platform resolved its roots")
        .logs()
        .to_path_buf();
    // config-history/ holds Stackly's own copy of every rendered file, the
    // drift baseline of spec 0029, which the first start writes.
    let owned = [
        paths.config_history.clone(),
        paths.data.clone(),
        paths.config.clone(),
        paths.temp.clone(),
        paths.run.clone(),
        paths.logs.clone(),
        logs_root,
        site.client_home.clone(),
    ];
    let outside = || -> Vec<String> {
        entries_under(site.root.path())
            .into_iter()
            .filter(|relative| {
                let path = site.root.path().join(relative);
                !owned.iter().any(|folder| path.starts_with(folder))
            })
            .collect()
    };
    let before = outside();

    let running = api.start_ready(made.id).await;
    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    let pid = pid_of(&running);
    site.subject(&ev, &site.version(http, &made.password));
    assert_two_listeners(pid, http, native);

    let settings = site.sql_ok(
        native,
        "default",
        Some(&made.password),
        "SELECT name, value FROM system.server_settings WHERE value LIKE '/%' ORDER BY name",
    );
    let inside = [
        paths.data.clone(),
        paths.temp.clone(),
        paths.config.clone(),
        paths.run.clone(),
    ];
    let mut keeper = Vec::new();
    for line in settings.lines() {
        let (name, value) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("a name and a value: {line}"));
        if KEEPER_PATHS.contains(&name) {
            keeper.push(name);
            continue;
        }
        assert!(
            inside
                .iter()
                .any(|folder| Path::new(value).starts_with(folder)),
            "{name} = {value} is outside the instance"
        );
    }
    keeper.sort_unstable();
    let mut expected = KEEPER_PATHS.to_vec();
    expected.sort_unstable();
    assert_eq!(keeper, expected, "{settings}");

    stop_instance(&api, made.id).await;
    eventually("the process to be gone", Duration::from_secs(60), || {
        !alive(pid)
    })
    .await;
    let again = api.start_ready(made.id).await;
    assert_two_listeners(
        pid_of(&again),
        port_of(&again, "http"),
        port_of(&again, "native"),
    );
    let after = outside();
    let added: Vec<&String> = after
        .iter()
        .filter(|entry| !before.contains(entry))
        .collect();
    assert!(added.is_empty(), "files outside the instance: {added:#?}");
    let home: Vec<String> = entries_under(&paths.temp)
        .into_iter()
        .filter(|entry| entry.starts_with(".clickhouse"))
        .collect();
    assert!(home.is_empty(), "no .clickhouse file in HOME: {home:?}");

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "two listeners 127.0.0.1:{http} and 127.0.0.1:{native}, no UDP, {} server settings paths inside the instance, nothing added outside it",
        settings.lines().count() - KEEPER_PATHS.len()
    ));
}

// covers: AC-6 (spec 0043)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn clickhouse_default_user_needs_the_keychain_password() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "the default user of a ClickHouse instance signs in with the Keychain password over HTTP and native TCP and is refused with a wrong or empty one; the password is in no file under the test root, no log line, and no argv, and users.xml and data/preprocessed_configs hold only its SHA-256",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "auth", json!({})).await;
    let running = api.start_ready(made.id).await;
    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    site.subject(&ev, &site.version(http, &made.password));

    assert_eq!(
        http_query(http, "default", &made.password, "SELECT 1"),
        (200, "1\n".to_owned())
    );
    assert_eq!(
        site.sql_ok(native, "default", Some(&made.password), "SELECT 1"),
        "1"
    );
    let wrong = format!("{}x", made.password);
    for password in [wrong.as_str(), ""] {
        let (status, body) = http_query(http, "default", password, "SELECT 1");
        assert_ne!(status, 200, "HTTP with `{password}`: {body}");
        assert!(
            body.contains("AUTHENTICATION_FAILED") || body.contains("Authentication failed"),
            "{body}"
        );
        let done = site.sql(native, "default", Some(password), "SELECT 1");
        assert!(!done.ok(), "native with `{password}`: {done:?}");
        assert!(done.stderr.contains("Authentication failed"), "{done:?}");
    }
    let done = site.sql(native, "default", None, "SELECT 1");
    assert!(!done.ok(), "native with no password: {done:?}");

    let hash = sha256_hex(&made.password);
    let users = std::fs::read_to_string(site.users_xml(made.id)).expect("users.xml is there");
    assert!(
        users.contains(&format!(
            "<password_sha256_hex>{hash}</password_sha256_hex>"
        )),
        "{users}"
    );
    let paths = site.instance_paths(made.id);
    let copies = files_holding(&paths.data.join("preprocessed_configs"), hash.as_bytes());
    assert!(
        !copies.is_empty(),
        "ClickHouse copies users.xml with the hash into preprocessed_configs"
    );
    let leaked = files_holding(site.root.path(), made.password.as_bytes());
    assert!(leaked.is_empty(), "the password is on disk: {leaked:#?}");
    let logged = log_lines(&api, made.id, &made.password).await;
    assert!(logged.is_empty(), "the password is in the log: {logged:#?}");
    let table = output_of("/bin/ps", &["-axww", "-o", "args="]);
    assert!(
        !table.contains(&made.password),
        "the password is in an argv"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    let leaked = files_holding(site.root.path(), made.password.as_bytes());
    assert!(
        leaked.is_empty(),
        "the password is on disk after the stop: {leaked:#?}"
    );
    ev.pass_with(
        "the Keychain password signed in over HTTP and native, a wrong and an empty one were refused, and only its hash was on disk",
    );
}

// covers: AC-6 (spec 0043)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn clickhouse_managed_users_can_do_nothing_else() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "stackly_readiness sees only the system database and is refused user tables, system.users, system.processes, file(), url(), CREATE, SETTINGS, and SYSTEM; stackly_reload has only its two SYSTEM RELOAD grants, sees no database, is refused user tables, SYSTEM SHUTDOWN, and other SYSTEM commands, and may run SELECT 1, numbers(), and SYSTEM RELOAD CONFIG",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "managed", json!({})).await;
    let running = api.start_ready(made.id).await;
    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    site.subject(&ev, &site.version(http, &made.password));
    site.exec(
        native,
        "default",
        Some(&made.password),
        "CREATE TABLE default.private (id UInt64) ENGINE = MergeTree ORDER BY id",
    );

    assert_eq!(
        site.sql_ok(native, READINESS_USER, None, "SHOW DATABASES"),
        "system"
    );
    assert_eq!(
        site.sql_ok(native, READINESS_USER, None, "SELECT dummy FROM system.one"),
        "0"
    );
    let url = format!("SELECT * FROM url('http://127.0.0.1:{http}/ping', RawBLOB)");
    for query in [
        "SELECT * FROM default.private",
        "SELECT name FROM system.users",
        "SELECT query FROM system.processes",
        "SELECT * FROM file('anything.txt', RawBLOB)",
        url.as_str(),
        "CREATE TABLE default.x (id UInt64) ENGINE = Memory",
        "SELECT 1 SETTINGS max_threads = 8",
        "SYSTEM RELOAD CONFIG",
    ] {
        let done = site.sql(native, READINESS_USER, None, query);
        assert!(!done.ok(), "{READINESS_USER} ran `{query}`: {done:?}");
    }

    let grants = site.sql_ok(native, RELOAD_USER, None, "SHOW GRANTS");
    assert_eq!(
        grants, "GRANT SYSTEM RELOAD CONFIG, SYSTEM RELOAD USERS ON *.* TO stackly_reload",
        "only the two SYSTEM RELOAD grants"
    );
    assert_eq!(site.sql_ok(native, RELOAD_USER, None, "SHOW DATABASES"), "");
    assert_eq!(site.sql_ok(native, RELOAD_USER, None, "SELECT 1"), "1");
    assert_eq!(
        site.sql_ok(native, RELOAD_USER, None, "SELECT count() FROM numbers(10)"),
        "10"
    );
    site.exec(native, RELOAD_USER, None, "SYSTEM RELOAD CONFIG");
    for query in [
        "SELECT * FROM default.private",
        "SELECT name FROM system.users",
        "SYSTEM SHUTDOWN",
        "SYSTEM DROP DNS CACHE",
        "SYSTEM FLUSH LOGS",
    ] {
        let done = site.sql(native, RELOAD_USER, None, query);
        assert!(!done.ok(), "{RELOAD_USER} ran `{query}`: {done:?}");
    }
    let still = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(pid_of(&still), pid_of(&running), "the server is still up");

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("both managed users were refused everything but their one job");
}

/// The four `SYSTEM STOP` commands that ClickHouse runs for any user when
/// no table is named: with no table the user may touch, they stop nothing.
const STOP_COMMANDS: [&str; 4] = [
    "SYSTEM STOP MERGES",
    "SYSTEM STOP MOVES",
    "SYSTEM STOP TTL MERGES",
    "SYSTEM STOP FETCHES",
];

/// The active parts of `default.parts`, as the `default` user sees them.
fn active_parts(site: &Site, native: u16, password: &str) -> String {
    site.sql_ok(
        native,
        "default",
        Some(password),
        "SELECT count() FROM system.parts WHERE database = 'default' AND table = 'parts' AND active",
    )
}

/// Inserts one part per row into `default.parts`, then merges with
/// `OPTIMIZE TABLE ... FINAL` as `default`, and returns the active parts.
fn insert_and_optimize(site: &Site, native: u16, password: &str, rows: &[u64]) -> String {
    for row in rows {
        site.exec(
            native,
            "default",
            Some(password),
            &format!("INSERT INTO default.parts VALUES ({row})"),
        );
    }
    site.exec(
        native,
        "default",
        Some(password),
        "OPTIMIZE TABLE default.parts FINAL",
    );
    active_parts(site, native, password)
}

// covers: AC-6 (spec 0043)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn the_managed_users_stop_commands_without_a_table_leave_merges_running() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "SYSTEM STOP MERGES, STOP MOVES, STOP TTL MERGES, and STOP FETCHES with no table name exit 0 for stackly_readiness and stackly_reload but stop nothing: OPTIMIZE TABLE default.parts FINAL as default still merges the parts to one, while the same commands naming default.parts are refused for both users, and the default user's own SYSTEM STOP MERGES default.parts does keep the parts apart",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "merges", json!({})).await;
    let running = api.start_ready(made.id).await;
    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    let password = made.password.as_str();
    site.subject(&ev, &site.version(http, password));
    site.exec(
        native,
        "default",
        Some(password),
        "CREATE TABLE default.parts (id UInt64) ENGINE = MergeTree ORDER BY id",
    );

    // The control: a stop that does apply keeps the parts apart, so the
    // part count below would see a stop that took effect.
    site.exec(
        native,
        "default",
        Some(password),
        "SYSTEM STOP MERGES default.parts",
    );
    for row in [1, 2, 3] {
        site.exec(
            native,
            "default",
            Some(password),
            &format!("INSERT INTO default.parts VALUES ({row})"),
        );
    }
    // With merges stopped, OPTIMIZE may end with ABORTED; only the parts
    // count matters here.
    let _ = site.sql(
        native,
        "default",
        Some(password),
        "OPTIMIZE TABLE default.parts FINAL",
    );
    assert_eq!(
        active_parts(&site, native, password),
        "3",
        "merges stay stopped for default.parts"
    );
    site.exec(
        native,
        "default",
        Some(password),
        "SYSTEM START MERGES default.parts",
    );
    assert_eq!(insert_and_optimize(&site, native, password, &[]), "1");

    let mut refusals = Vec::new();
    for user in [READINESS_USER, RELOAD_USER] {
        for command in STOP_COMMANDS {
            let done = site.sql(native, user, None, command);
            assert!(
                done.ok(),
                "`{command}` with no table as {user} exits 0: {done:?}"
            );
            let named = format!("{command} default.parts");
            let refused = site.sql(native, user, None, &named);
            assert!(!refused.ok(), "{user} ran `{named}`: {refused:?}");
            refusals.push(refused.code);
        }
        let merged = insert_and_optimize(&site, native, password, &[10, 11, 12]);
        assert_eq!(
            merged, "1",
            "the stop commands of {user} with no table stopped nothing"
        );
    }
    assert_eq!(
        site.sql_ok(
            native,
            "default",
            Some(password),
            "SELECT count() FROM default.parts"
        ),
        "9"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "the four SYSTEM STOP commands without a table exited 0 for both managed users and OPTIMIZE FINAL still merged to one part; the named forms exited {refusals:?}"
    ));
}
