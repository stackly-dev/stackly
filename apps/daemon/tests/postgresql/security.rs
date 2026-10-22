//! Who can log in, with what, and where the password goes (spec 0019 AC-4,
//! AC-6, AC-7).

use std::sync::Arc;

use serde_json::json;
use stackly_protocol::codes::secret;
use stackly_testkit::SecretOp;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{
    PG17, PG18, Pg, Psql, Site, Via, create_instance, create_params, create_with, files_holding,
    new_password, output_of, stop_and_shutdown,
};
use crate::support::LogRecorder;

fn hba_methods(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .flat_map(|line| {
            line.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

async fn has_no_trust_and_its_password_stays_out_of_argv_files_and_logs(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
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
    let made = create_instance(&site, &api, "secure").await;
    let running = api.start_ready(made.id).await;
    let paths = site.instance_paths(made.id);
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    for file in [
        paths.config.join("pg_hba.conf"),
        paths.data.join("pg_hba.conf"),
    ] {
        let words = hba_methods(&file);
        assert!(!words.is_empty(), "{} has rules", file.display());
        assert!(
            !words.iter().any(|word| word == "trust"),
            "{} has a trust rule",
            file.display()
        );
    }

    for via in [Via::Socket, Via::Tcp] {
        let without = psql.run(via, "postgres", None, "postgres", "SELECT 1");
        assert!(
            !without.ok,
            "{via:?}: the superuser logged in without a password"
        );
        let wrong = psql.run(
            via,
            "postgres",
            Some("not the password"),
            "postgres",
            "SELECT 1",
        );
        assert!(!wrong.ok, "{via:?}: a wrong password was accepted");
        let with = psql.run(
            via,
            "postgres",
            Some(&made.password),
            "postgres",
            "SELECT 1",
        );
        assert!(with.ok, "{via:?}: {with:?}");
        assert_eq!(with.stdout, "1");
    }

    let readiness_tcp = psql.run(Via::Tcp, "stackly_readiness", None, "postgres", "SELECT 1");
    assert!(!readiness_tcp.ok, "the readiness role came in over TCP");
    let readiness_other = psql.run(
        Via::Socket,
        "stackly_readiness",
        None,
        "template1",
        "SELECT 1",
    );
    assert!(!readiness_other.ok, "the readiness role reached template1");
    let readiness_ok = psql.run(
        Via::Socket,
        "stackly_readiness",
        None,
        "postgres",
        "SELECT 1",
    );
    assert!(readiness_ok.ok, "{readiness_ok:?}");

    // `all` in pg_hba.conf never covers a physical replication connection, and
    // no rule names `replication`. A logical one (`replication=database`)
    // is matched by the rule of its database, so it is the same login as any
    // other and the readiness role cannot use it.
    let physical = psql.conninfo(
        &format!(
            "{} user=postgres password={} dbname=postgres replication=true",
            psql.socket_conninfo(),
            made.password
        ),
        "IDENTIFY_SYSTEM",
    );
    assert!(
        !physical.ok,
        "a physical replication connection was accepted"
    );
    assert!(
        physical
            .stderr
            .contains("no pg_hba.conf entry for replication"),
        "{physical:?}"
    );
    let logical = psql.conninfo(
        &format!(
            "{} user=stackly_readiness dbname=postgres replication=database",
            psql.socket_conninfo(),
        ),
        "IDENTIFY_SYSTEM",
    );
    assert!(
        !logical.ok,
        "the readiness role opened a replication connection"
    );
    assert!(
        logical
            .stderr
            .contains("permission denied to start WAL sender"),
        "{logical:?}"
    );
    let logical_tcp = psql.conninfo(
        &format!(
            "host=127.0.0.1 port={} user=stackly_readiness dbname=postgres replication=database",
            made.port
        ),
        "IDENTIFY_SYSTEM",
    );
    assert!(
        !logical_tcp.ok,
        "the readiness role opened a replication connection over TCP"
    );
    assert!(
        logical_tcp
            .stderr
            .contains("pg_hba.conf rejects connection"),
        "{logical_tcp:?}"
    );

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
    assert_eq!(running.software, "postgresql");

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(
        "no trust rule, no passwordless login, replication=true refused, the readiness role kept out of replication=database, no password on disk or in argv",
    );
}

async fn takes_a_password_with_quotes_spaces_and_backslashes_and_refuses_a_line_break(
    pg: Pg,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    let odd = format!("{} 'x\"y\\z", new_password());
    let made = create_with(&site, &api, "odd", &odd, json!({})).await;
    api.start_ready(made.id).await;
    let psql: Psql = site.psql(made.id, made.port);
    let version = psql.scalar(&odd, "SHOW server_version");
    site.subject(&ev, &version);
    for via in [Via::Socket, Via::Tcp] {
        let ran = psql.run(via, "postgres", Some(&odd), "postgres", "SELECT 1");
        assert!(ran.ok, "{via:?}: {ran:?}");
    }

    let writes = |site: &Site| site.secrets.count(SecretOp::Write);
    let before = writes(&site);
    let multi = format!("{}\nsecond line", new_password());
    // The create refuses the value before it starts an operation.
    let refused = api
        .create(create_params(
            &site,
            "multi",
            stackly_protocol::instance::PortRequest::Auto,
            &multi,
            json!({}),
        ))
        .await
        .expect_err("a password with a line break is refused");
    assert_eq!(refused.code(), secret::INVALID_VALUE.code, "{refused:?}");
    assert_eq!(refused.detail("field"), Some("superuserPassword"));
    assert_eq!(refused.detail("reason"), Some("control_characters"));
    assert_eq!(writes(&site), before, "the keychain was not written");
    assert!(
        !format!("{refused:?}").contains("second line"),
        "the refusal quotes the value"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("the odd password logged in over the socket and TCP, the line break was refused");
}

async fn fixes_locale_encoding_and_time_zone_whatever_the_daemon_environment(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.binary(&[
        ("LANG", "de_DE.UTF-8"),
        ("LC_ALL", "de_DE.UTF-8"),
        ("TZ", "Asia/Jakarta"),
    ]);
    let api = site.binary_api(&daemon).await;
    let made = create_instance(&site, &api, "locale").await;
    api.start_ready(made.id).await;
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    let rows = psql.scalar(
        &made.password,
        "SELECT datname || '|' || datlocprovider::text || '|' || datlocale || '|' || pg_encoding_to_char(encoding) || '|' || datcollate || '|' || datctype FROM pg_database WHERE datname IN ('postgres', 'template1') ORDER BY datname",
    );
    assert_eq!(
        rows,
        "postgres|b|C.UTF-8|UTF8|C|C\ntemplate1|b|C.UTF-8|UTF8|C|C"
    );
    for (setting, expected) in [
        ("data_checksums", "on"),
        ("TimeZone", "UTC"),
        ("log_timezone", "UTC"),
        ("lc_messages", "C"),
    ] {
        assert_eq!(
            psql.scalar(&made.password, &format!("SHOW {setting}")),
            expected,
            "{setting}"
        );
    }

    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    daemon.stop();
    ev.pass_with("postgres|b|C.UTF-8|UTF8|C|C under de_DE and Asia/Jakarta");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_has_no_trust_and_its_password_stays_out_of_argv_files_and_logs() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "VAL-018"],
        kind = Kind::Native,
        expected = "neither pg_hba.conf has a trust line, the superuser cannot log in without its password over the socket or TCP, the readiness role cannot use TCP or another database, a replication=true connection is refused and the readiness role cannot open a replication=database connection, and the password is in no argv, file, or log line",
    );
    has_no_trust_and_its_password_stays_out_of_argv_files_and_logs(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_has_no_trust_and_its_password_stays_out_of_argv_files_and_logs() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "VAL-018"],
        kind = Kind::Native,
        expected = "neither pg_hba.conf has a trust line, the superuser cannot log in without its password over the socket or TCP, the readiness role cannot use TCP or another database, a replication=true connection is refused and the readiness role cannot open a replication=database connection, and the password is in no argv, file, or log line",
    );
    has_no_trust_and_its_password_stays_out_of_argv_files_and_logs(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_takes_a_password_with_quotes_spaces_and_backslashes_and_refuses_a_line_break()
 {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "a superuser password with a space, a single quote, a double quote, and a backslash reaches initdb whole and logs in, and a password with a line break is refused before anything is stored",
    );
    takes_a_password_with_quotes_spaces_and_backslashes_and_refuses_a_line_break(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_takes_a_password_with_quotes_spaces_and_backslashes_and_refuses_a_line_break()
 {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "a superuser password with a space, a single quote, a double quote, and a backslash reaches initdb whole and logs in, and a password with a line break is refused before anything is stored",
    );
    takes_a_password_with_quotes_spaces_and_backslashes_and_refuses_a_line_break(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_fixes_locale_encoding_and_time_zone_whatever_the_daemon_environment() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "with LANG=de_DE.UTF-8 and TZ=Asia/Jakarta in the daemon, postgres and template1 are builtin C.UTF-8, UTF8, collate and ctype C, checksums are on, the time zones are UTC, and lc_messages is C",
    );
    fixes_locale_encoding_and_time_zone_whatever_the_daemon_environment(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_fixes_locale_encoding_and_time_zone_whatever_the_daemon_environment() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "with LANG=de_DE.UTF-8 and TZ=Asia/Jakarta in the daemon, postgres and template1 are builtin C.UTF-8, UTF8, collate and ctype C, checksums are on, the time zones are UTC, and lc_messages is C",
    );
    fixes_locale_encoding_and_time_zone_whatever_the_daemon_environment(pg, ev).await;
}
