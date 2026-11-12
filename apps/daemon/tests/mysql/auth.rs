//! Who can sign in, with what, and where the password goes (spec 0042 AC-4,
//! VAL-018).

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::json;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{
    MY84, MY97, My, Mysql, READINESS, ROOT, Site, Via, create_generated, create_with,
    files_holding, new_password, output_of, stop_and_shutdown,
};
use crate::support::LogRecorder;

/// The privileges `SHOW GRANTS` lists for `account`, as one set: every name
/// of every `GRANT ... ON` line, plus `PROXY` lines as `PROXY ON <target>`.
fn privileges(mysql: &Mysql, password: &str, account: &str) -> BTreeSet<String> {
    let grants = mysql.scalar(password, &format!("SHOW GRANTS FOR {account}"));
    let mut names = BTreeSet::new();
    for line in grants.lines() {
        let rest = line.strip_prefix("GRANT ").expect("a grant line");
        let (list, target) = rest.split_once(" ON ").expect("a grant names its target");
        if list == "PROXY" {
            let target = target.split(" TO ").next().unwrap_or_default();
            names.insert(format!("PROXY ON {target}"));
            continue;
        }
        let target = target.split(" TO ").next().unwrap_or_default();
        for name in list.split(',') {
            names.insert(format!("{} ON {target}", name.trim()));
        }
    }
    names
}

async fn has_three_root_accounts_and_a_readiness_account_that_only_reaches_the_socket(
    my: My,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let recorder = Arc::new(LogRecorder::default());
    let daemon = site
        .in_process(|config| {
            config
                .extra_log_sinks
                .push(Arc::clone(&recorder) as Arc<dyn stackly_core::supervisor::LogSink>);
        })
        .await;
    let api = site.api().await;
    let made = create_generated(&site, &api, "auth").await;
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let accounts = mysql.scalar(
        &made.password,
        "SELECT CONCAT(user, '@', host) FROM mysql.user WHERE account_locked = 'N' ORDER BY 1",
    );
    let mut accounts: Vec<&str> = accounts.lines().collect();
    accounts.sort_unstable();
    assert_eq!(
        accounts,
        [
            "root@127.0.0.1",
            "root@::1",
            "root@localhost",
            "stackly_readiness@localhost"
        ]
    );

    let local = privileges(&mysql, &made.password, "'root'@'localhost'");
    for host in ["127.0.0.1", "::1"] {
        let ip = privileges(&mysql, &made.password, &format!("'root'@'{host}'"));
        let only_local: Vec<&String> = local.difference(&ip).collect();
        assert_eq!(
            only_local,
            ["AUDIT_ABORT_EXEMPT ON *.*", "FIREWALL_EXEMPT ON *.*"],
            "{host}"
        );
        assert!(ip.is_subset(&local), "{host}: {:?}", ip.difference(&local));
    }
    let readiness = mysql.scalar(
        &made.password,
        "SHOW GRANTS FOR 'stackly_readiness'@'localhost'",
    );
    assert_eq!(
        readiness,
        "GRANT USAGE ON *.* TO `stackly_readiness`@`localhost`"
    );
    assert_eq!(
        mysql.scalar(
            &made.password,
            "SELECT max_user_connections FROM mysql.user WHERE user = 'stackly_readiness'"
        ),
        "3"
    );

    for via in [Via::Socket, Via::Tcp] {
        let without = mysql.run(via, ROOT, None, "SELECT 1");
        assert!(!without.ok, "{via:?}: root signed in without a password");
        let wrong = mysql.run(via, ROOT, Some("not the password"), "SELECT 1");
        assert!(!wrong.ok, "{via:?}: a wrong password was accepted");
        let with = mysql.run(via, ROOT, Some(&made.password), "SELECT 1");
        assert!(with.ok, "{via:?}: {with:?}");
        assert_eq!(with.stdout, "1");
    }
    let readiness_tcp = mysql.run(Via::Tcp, READINESS, None, "SELECT 1");
    assert!(!readiness_tcp.ok, "the readiness account came in over TCP");
    assert!(
        readiness_tcp.stderr.contains("Access denied"),
        "{readiness_tcp:?}"
    );
    let readiness_socket = mysql.run(Via::Socket, READINESS, None, "SELECT 1");
    assert!(readiness_socket.ok, "{readiness_socket:?}");
    let create = mysql.run(Via::Socket, READINESS, None, "CREATE DATABASE x");
    assert!(!create.ok, "the readiness account created a database");

    let processes = output_of("/bin/ps", &["-axww", "-o", "command="]);
    assert!(
        !processes.contains(&made.password),
        "the password is in the argv of a process"
    );
    for held in [site.root.path(), site.prepared.layout().roots().runtime()] {
        let found = files_holding(held, &made.password);
        assert!(found.is_empty(), "the password is in {found:?}");
    }
    assert!(
        recorder
            .text_of(made.id)
            .iter()
            .all(|line| !line.contains(&made.password)),
        "the password is in a log line"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(
        "four unlocked accounts, the IP roots lack only AUDIT_ABORT_EXEMPT and FIREWALL_EXEMPT, root needs its password on both paths, stackly_readiness only reaches the socket with USAGE, no password on disk, in argv, or in a log line",
    );
}

// covers: AC-4 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_has_three_root_accounts_and_a_readiness_account_that_only_reaches_the_socket() {
    let ev = evidence!(
        covers = ["REQ-055", "VAL-018"],
        kind = Kind::Native,
        expected = "after init the unlocked accounts are root@localhost, root@127.0.0.1, root@::1, and stackly_readiness@localhost; the IP roots have the grants of root@localhost but AUDIT_ABORT_EXEMPT and FIREWALL_EXEMPT; root is refused without its password and accepted with it over the socket and TCP; stackly_readiness is refused over TCP, has only USAGE, cannot CREATE DATABASE, and has MAX_USER_CONNECTIONS 3; the password is in no argv, file, or log line",
    );
    has_three_root_accounts_and_a_readiness_account_that_only_reaches_the_socket(MY84, ev).await;
}

// covers: AC-4 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_has_three_root_accounts_and_a_readiness_account_that_only_reaches_the_socket() {
    let ev = evidence!(
        covers = ["REQ-055", "VAL-018"],
        kind = Kind::Native,
        expected = "after init the unlocked accounts are root@localhost, root@127.0.0.1, root@::1, and stackly_readiness@localhost; the IP roots have the grants of root@localhost but AUDIT_ABORT_EXEMPT and FIREWALL_EXEMPT; root is refused without its password and accepted with it over the socket and TCP; stackly_readiness is refused over TCP, has only USAGE, cannot CREATE DATABASE, and has MAX_USER_CONNECTIONS 3; the password is in no argv, file, or log line",
    );
    has_three_root_accounts_and_a_readiness_account_that_only_reaches_the_socket(MY97, ev).await;
}

async fn takes_a_password_with_quotes_backslashes_spaces_and_unicode(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    // The binary daemon writes its log under the test root, so the scan of
    // the root below covers the daemon log too.
    let daemon = site.binary(&[]);
    let api = site.binary_api(&daemon).await;
    let odd = format!("pa'ss\"w\\ord ü空 x {}", new_password());
    let made = create_with(&site, &api, "odd", &odd, json!({})).await;
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&odd, "SELECT VERSION()");
    site.subject(&ev, &version);
    for via in [Via::Socket, Via::Tcp] {
        let ran = mysql.run(via, ROOT, Some(&odd), "SELECT 1");
        assert!(ran.ok, "{via:?}: {ran:?}");
    }
    let processes = output_of("/bin/ps", &["-axww", "-o", "command="]);
    assert!(!processes.contains(&odd), "the password is in an argv");
    for held in [site.root.path(), site.prepared.layout().roots().runtime()] {
        let found = files_holding(held, &odd);
        assert!(found.is_empty(), "the password is in {found:?}");
    }
    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    daemon.stop();
    ev.pass_with("the odd password signed in over the socket and TCP and is in no file or argv");
}

// covers: AC-4 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_takes_a_password_with_quotes_backslashes_spaces_and_unicode() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "a root password with a single quote, a double quote, a backslash, spaces, and Unicode letters signs in over the socket and TCP, and it is in no argv and no file under the test root, the daemon log included",
    );
    takes_a_password_with_quotes_backslashes_spaces_and_unicode(MY84, ev).await;
}

// covers: AC-4 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_takes_a_password_with_quotes_backslashes_spaces_and_unicode() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "a root password with a single quote, a double quote, a backslash, spaces, and Unicode letters signs in over the socket and TCP, and it is in no argv and no file under the test root, the daemon log included",
    );
    takes_a_password_with_quotes_backslashes_spaces_and_unicode(MY97, ev).await;
}
