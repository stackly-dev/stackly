//! `stackly instance connection` and the detail rows of `inspect` against a
//! scripted daemon (spec 0021 AC-6, AC-7, AC-18): the masked default, the
//! choice of an entry, the refusals, `--all`, the password only on
//! `--show-password`, and the four fixture triples.

use stackly_protocol::instance::{InstancePage, InstanceSummary};
use stackly_protocol::secret::{RevealSecretReply, SecretText};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{detail, envelope, golden, run};
use crate::data::{self, Triple};

/// A password with every character `compose` must encode.
const PASSWORD: &str = "p@ss word/%?#";
/// The same, encoded.
const ENCODED: &str = "p%40ss%20word%2F%25%3F%23";

/// A daemon that knows the instance of `triple`.
fn serving(triple: &Triple) -> (TestRoot, ScriptedDaemon) {
    serving_with(triple, |_| Vec::new())
}

/// The same, with `extra` answering anything else, such as `secret.reveal`.
fn serving_with(
    triple: &Triple,
    extra: impl Fn(&Call) -> Vec<Step> + Send + Sync + 'static,
) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let instance: InstanceSummary = triple.instance.clone();
    let detail = triple.detail.clone();
    let usage = triple.data_usage;
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
            items: vec![instance.clone()],
            next: None,
        })))],
        Call::InstanceGet(_) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
        Call::InstanceGetDetail(_) => {
            vec![Step::Reply(Reply::InstanceGetDetail(Box::new(
                detail.clone(),
            )))]
        }
        Call::InstanceGetDataUsage(_) => {
            vec![Step::Reply(Reply::InstanceGetDataUsage(
                stackly_protocol::instance::DataUsageReply { usage },
            ))]
        }
        other => extra(other),
    });
    (root, daemon)
}

/// A reveal handler that answers every reference with `PASSWORD`.
fn reveals(call: &Call) -> Vec<Step> {
    match call {
        Call::SecretReveal(params) => vec![Step::Reply(Reply::SecretReveal(RevealSecretReply {
            reference: params.reference.clone(),
            value: SecretText::new(PASSWORD.to_owned()),
        }))],
        _ => Vec::new(),
    }
}

fn reveal_calls(daemon: &ScriptedDaemon) -> Vec<serde_json::Value> {
    daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "secret.reveal")
        .map(|logged| logged.raw)
        .collect()
}

const PG_MASKED: &str = "postgresql://postgres:****@127.0.0.1:5432/postgres";

// covers: AC-6
#[test]
fn the_default_writes_the_masked_first_string_and_nothing_else_and_never_reveals() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "stackly instance connection writes exactly the masked first string and a newline, with nothing on stderr and no secret.reveal call",
    );
    let triple = data::triple("postgresql");
    let (root, daemon) = serving(&triple);

    let by_name = run(root.path(), &["instance", "connection", "orders-db"]);
    let by_id = run(
        root.path(),
        &["instance", "connection", &triple.instance.id.to_string()],
    );

    assert_eq!(by_name.code, Some(0), "{by_name:?}");
    assert_eq!(by_name.stdout, format!("{PG_MASKED}\n"));
    assert_eq!(by_name.stderr, "");
    assert_eq!(by_id.stdout, by_name.stdout);
    assert!(reveal_calls(&daemon).is_empty());
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.list",
            "instance.get_detail",
            "daemon.handshake",
            "instance.get_detail",
            "instance.get"
        ]
    );
    ev.pass();
}

// covers: AC-6
#[test]
fn format_picks_an_entry_and_an_unknown_name_exits_10_with_the_names_it_could_take() {
    let triple = data::triple("postgresql");
    let (root, _daemon) = serving(&triple);

    let socket = run(
        root.path(),
        &["instance", "connection", "orders-db", "--format", "socket"],
    );
    let nope = run(
        root.path(),
        &[
            "instance",
            "connection",
            "orders-db",
            "--format",
            "nope",
            "--json",
        ],
    );

    assert_eq!(socket.code, Some(0), "{socket:?}");
    assert!(
        socket
            .stdout
            .starts_with("postgresql://postgres:****@/postgres?host="),
        "{socket:?}"
    );
    assert_eq!(nope.code, Some(10), "{nope:?}");
    assert_eq!(nope.stderr, "");
    golden::check("instance_connection_not_found.json", &nope.stdout);
    let value = envelope(&nope);
    assert_eq!(value["error"]["code"], "cli.connection_not_found");
    assert_eq!(detail(&value["error"], "format").as_deref(), Some("nope"));
    assert_eq!(
        detail(&value["error"], "available").as_deref(),
        Some("uri,socket")
    );
}

// covers: AC-6
#[test]
fn an_entry_that_is_not_available_exits_3_with_its_reason_and_all_lists_it() {
    let triple = data::triple("nats");
    let (root, _daemon) = serving(&triple);

    let one = run(root.path(), &["instance", "connection", "queue"]);
    let named = run(
        root.path(),
        &[
            "instance",
            "connection",
            "queue",
            "--format",
            "monitoring",
            "--json",
        ],
    );
    let all = run(root.path(), &["instance", "connection", "queue", "--all"]);

    assert_eq!(one.code, Some(3), "{one:?}");
    assert_eq!(one.stdout, "");
    assert!(
        one.stderr
            .contains("the uri connection is not available yet"),
        "{one:?}"
    );
    assert!(one.stderr.contains("cli.connection_unavailable"), "{one:?}");
    assert!(
        one.stderr.contains("start the instance to assign its port"),
        "{one:?}"
    );
    assert_eq!(named.code, Some(3), "{named:?}");
    let value = envelope(&named);
    assert_eq!(
        detail(&value["error"], "format").as_deref(),
        Some("monitoring")
    );
    assert_eq!(
        detail(&value["error"], "reason").as_deref(),
        Some("port_not_assigned")
    );
    assert_eq!(all.code, Some(0), "{all:?}");
    assert_eq!(
        all.stdout,
        "uri         unavailable (port_not_assigned)\nmonitoring  unavailable (port_not_assigned)\n"
    );
}

// covers: AC-6
#[test]
fn an_unrecognized_state_is_unavailable_with_the_hint_to_update() {
    let mut triple = data::triple("postgresql");
    triple.detail.connections[0].state =
        stackly_protocol::instance::ConnectionAvailability::Unrecognized;
    triple.detail.connections[0].masked = None;
    triple.detail.connections[0].parts.clear();
    let (root, _daemon) = serving(&triple);

    let run = run(root.path(), &["instance", "connection", "orders-db"]);

    assert_eq!(run.code, Some(3), "{run:?}");
    assert!(run.stderr.contains("update Stackly"), "{run:?}");
}

// covers: AC-6
#[test]
fn a_provider_without_a_connection_block_exits_10_with_no_names_available() {
    let mut triple = data::triple("postgresql");
    triple.detail.connections.clear();
    let (root, _daemon) = serving(&triple);

    let json = run(
        root.path(),
        &["instance", "connection", "orders-db", "--json"],
    );
    let named = run(
        root.path(),
        &[
            "instance",
            "connection",
            "orders-db",
            "--format",
            "uri",
            "--json",
        ],
    );

    for run in [&json, &named] {
        assert_eq!(run.code, Some(10), "{run:?}");
        let value = envelope(run);
        assert_eq!(value["error"]["code"], "cli.connection_not_found");
        assert_eq!(detail(&value["error"], "available").as_deref(), Some(""));
    }
    assert_eq!(
        envelope(&json)["error"]["message"],
        "this provider describes no connection strings"
    );
}

// covers: AC-6
#[test]
fn a_package_that_cannot_be_read_fails_every_form_with_package_unavailable_and_reveals_nothing() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-016"],
        kind = Kind::Fixture,
        expected = "with packageReadable false every form of stackly instance connection exits 3 with cli.connection_unavailable and reason package_unavailable, writes no connection on stdout, calls no secret.reveal, and names format only when --format was sent",
    );
    let triple = data::triple("package-unreadable");
    let (root, daemon) = serving_with(&triple, reveals);
    let forms: [(&[&str], bool); 6] = [
        (&["instance", "connection", "secretive"], false),
        (&["instance", "connection", "secretive", "--json"], false),
        (
            &["instance", "connection", "secretive", "--format", "uri"],
            true,
        ),
        (
            &[
                "instance",
                "connection",
                "secretive",
                "--format",
                "uri",
                "--json",
            ],
            true,
        ),
        (&["instance", "connection", "secretive", "--all"], false),
        (
            &["instance", "connection", "secretive", "--all", "--json"],
            false,
        ),
    ];

    for (args, has_format) in forms {
        let outcome = run(root.path(), args);
        assert_eq!(outcome.code, Some(3), "{args:?}: {outcome:?}");
        if args.contains(&"--json") {
            let value = envelope(&outcome);
            assert_eq!(value["error"]["code"], "cli.connection_unavailable");
            assert_eq!(
                detail(&value["error"], "reason").as_deref(),
                Some("package_unavailable")
            );
            assert_eq!(detail(&value["error"], "format").is_some(), has_format);
            assert!(value["result"].is_null(), "{args:?}: {outcome:?}");
        } else {
            assert_eq!(outcome.stdout, "", "{args:?}");
            assert!(
                outcome
                    .stderr
                    .contains("the provider package of this instance can't be read"),
                "{args:?}: {outcome:?}"
            );
        }
    }
    let with_password = run(
        root.path(),
        &["instance", "connection", "secretive", "--show-password"],
    );
    assert_eq!(with_password.code, Some(3), "{with_password:?}");
    assert_eq!(with_password.stdout, "");
    assert!(reveal_calls(&daemon).is_empty());
    golden::check(
        "instance_connection_package_unreadable.json",
        &run(
            root.path(),
            &["instance", "connection", "secretive", "--json"],
        )
        .stdout,
    );
    ev.pass();
}

// covers: AC-6
#[test]
fn all_writes_one_line_per_entry_and_json_holds_every_entry() {
    let triple = data::triple("postgresql");
    let (root, _daemon) = serving(&triple);

    let text = run(
        root.path(),
        &["instance", "connection", "orders-db", "--all"],
    );
    let json = run(
        root.path(),
        &["instance", "connection", "orders-db", "--all", "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("instance_connection_all.txt", &text.stdout);
    golden::check("instance_connection_all.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["command"], "instance connection");
    assert_eq!(value["result"]["instance"]["name"], "orders-db");
    assert_eq!(value["result"]["connections"].as_array().unwrap().len(), 2);
    assert!(value["result"].get("value").is_none());
}

// covers: AC-6
#[test]
fn all_cannot_be_combined_with_format_or_show_password() {
    let root = TestRoot::new().expect("a test root is made");

    let with_format = run(
        root.path(),
        &[
            "instance",
            "connection",
            "orders-db",
            "--all",
            "--format",
            "uri",
            "--json",
        ],
    );
    let with_password = run(
        root.path(),
        &[
            "instance",
            "connection",
            "orders-db",
            "--all",
            "--show-password",
            "--json",
        ],
    );

    for run in [&with_format, &with_password] {
        assert_eq!(run.code, Some(2), "{run:?}");
        assert_eq!(envelope(run)["error"]["code"], "cli.usage");
    }
}

// covers: AC-6
#[test]
fn show_password_reveals_once_per_reference_for_the_show_purpose_and_composes_the_string() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-016", "AC-016"],
        kind = Kind::Fixture,
        expected = "--show-password sends secret.reveal with purpose show once per distinct reference, writes the whole string with the encoded password, and puts it in value of --json next to the masked connection",
    );
    let triple = data::triple("postgresql");
    let (root, daemon) = serving_with(&triple, reveals);

    let text = run(
        root.path(),
        &["instance", "connection", "orders-db", "--show-password"],
    );
    let json = run(
        root.path(),
        &[
            "instance",
            "connection",
            "orders-db",
            "--show-password",
            "--json",
        ],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout,
        format!("postgresql://postgres:{ENCODED}@127.0.0.1:5432/postgres\n")
    );
    let value = envelope(&json);
    assert_eq!(
        value["result"]["value"],
        format!("postgresql://postgres:{ENCODED}@127.0.0.1:5432/postgres")
    );
    assert_eq!(value["result"]["connection"]["masked"], PG_MASKED);
    let reveals = reveal_calls(&daemon);
    assert_eq!(reveals.len(), 2, "one per run: {reveals:?}");
    for call in &reveals {
        assert_eq!(call["call"]["content"]["purpose"], "show");
        assert_eq!(
            call["call"]["content"]["reference"],
            triple.detail.secrets[0].reference
        );
    }
    ev.pass();
}

// covers: AC-6
#[test]
fn the_socket_string_reveals_the_same_reference_once_and_an_entry_without_a_secret_reveals_none() {
    let triple = data::triple("postgresql");
    let (root, daemon) = serving_with(&triple, reveals);
    let both = run(
        root.path(),
        &[
            "instance",
            "connection",
            "orders-db",
            "--format",
            "socket",
            "--show-password",
        ],
    );
    assert_eq!(both.code, Some(0), "{both:?}");
    assert!(
        both.stdout
            .contains(&format!("postgres:{ENCODED}@/postgres"))
    );
    assert_eq!(reveal_calls(&daemon).len(), 1);

    let mut plain = data::triple("postgresql");
    plain.detail.connections.truncate(1);
    plain.detail.connections[0].parts =
        vec![stackly_protocol::instance::connection::ConnectionPart::text("nats://127.0.0.1:4222")];
    plain.detail.connections[0].masked = Some("nats://127.0.0.1:4222".to_owned());
    let (root2, daemon2) = serving_with(&plain, reveals);
    let none = run(
        root2.path(),
        &["instance", "connection", "orders-db", "--show-password"],
    );
    assert_eq!(none.code, Some(0), "{none:?}");
    assert_eq!(none.stdout, "nats://127.0.0.1:4222\n");
    assert!(reveal_calls(&daemon2).is_empty());
}

// covers: AC-6
#[test]
fn a_reveal_that_fails_stops_the_command_with_that_error_and_writes_nothing_on_stdout() {
    let triple = data::triple("postgresql");
    let (root, _daemon) = serving_with(&triple, |call| match call {
        Call::SecretReveal(_) => vec![Step::Fail(error(
            "secret.not_found",
            "not_found",
            "no secret has this reference",
        ))],
        _ => Vec::new(),
    });

    let text = run(
        root.path(),
        &["instance", "connection", "orders-db", "--show-password"],
    );
    let json = run(
        root.path(),
        &[
            "instance",
            "connection",
            "orders-db",
            "--show-password",
            "--json",
        ],
    );

    assert_eq!(text.code, Some(10), "{text:?}");
    assert_eq!(text.stdout, "");
    assert_eq!(json.code, Some(10), "{json:?}");
    assert!(envelope(&json)["result"].is_null());
}

// covers: AC-6
#[test]
fn a_port_that_can_change_adds_a_note_on_stderr_in_text_mode_only() {
    let mut triple = data::triple("postgresql");
    triple.detail.connections[0].port_may_change = true;
    let (root, _daemon) = serving(&triple);

    let text = run(root.path(), &["instance", "connection", "orders-db"]);
    let json = run(
        root.path(),
        &["instance", "connection", "orders-db", "--json"],
    );
    let quiet = run(
        root.path(),
        &["instance", "connection", "orders-db", "--quiet"],
    );

    assert_eq!(
        text.stderr,
        "note: the port can change the next time the instance starts\n"
    );
    assert_eq!(text.stdout, format!("{PG_MASKED}\n"));
    assert_eq!(json.stderr, "");
    assert_eq!(quiet.stderr, "");
}

// covers: AC-18
#[test]
fn no_default_output_holds_the_password() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Fixture,
        expected = "stackly instance connection and inspect without --show-password write no part of the password on stdout or stderr, in text and in --json, and never call secret.reveal",
    );
    let triple = data::triple("postgresql");
    let (root, daemon) = serving_with(&triple, reveals);
    let runs = [
        run(root.path(), &["instance", "connection", "orders-db"]),
        run(
            root.path(),
            &["instance", "connection", "orders-db", "--json"],
        ),
        run(
            root.path(),
            &["instance", "connection", "orders-db", "--all", "--json"],
        ),
        run(root.path(), &["instance", "inspect", "orders-db"]),
        run(root.path(), &["instance", "inspect", "orders-db", "--json"]),
    ];

    for outcome in &runs {
        assert_eq!(outcome.code, Some(0), "{outcome:?}");
        for text in [&outcome.stdout, &outcome.stderr] {
            assert!(!text.contains(PASSWORD), "{outcome:?}");
            assert!(!text.contains(&PASSWORD[..4]), "{outcome:?}");
            assert!(!text.contains(ENCODED), "{outcome:?}");
        }
    }
    assert!(reveal_calls(&daemon).is_empty());
    ev.pass();
}

// covers: AC-7
#[test]
fn inspect_writes_the_detail_rows_of_each_fixture_triple() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "stackly instance inspect of each of the four fixture triples writes the Digest, Data, Data size, Autostart, Dependents, Secrets, and CONNECTION rows, or the one line of an unreadable package, as the goldens hold them",
    );
    for (name, who) in [
        ("postgresql", "orders-db"),
        ("nats", "queue"),
        ("unavailable", "locked-db"),
        ("package-unreadable", "secretive"),
    ] {
        let triple = data::triple(name);
        let (root, _daemon) = serving(&triple);

        let text = run(root.path(), &["instance", "inspect", who]);
        let json = run(root.path(), &["instance", "inspect", who, "--json"]);

        assert_eq!(text.code, Some(0), "{name}: {text:?}");
        golden::check(
            &format!("instance_inspect_{}.txt", name.replace('-', "_")),
            &text.stdout,
        );
        let value = envelope(&json);
        assert_eq!(
            value["result"]["detail"],
            serde_json::to_value(&triple.detail).unwrap(),
            "{name}"
        );
        assert_eq!(
            value["result"]["dataUsage"],
            serde_json::to_value(triple.data_usage).unwrap(),
            "{name}"
        );
    }
    ev.pass();
}

// covers: AC-7
#[test]
fn inspect_shows_the_size_of_the_data_folder_as_the_table_of_the_spec_says() {
    let mut triple = data::triple("postgresql");
    let cases = [
        (Some((999, true)), "999 bytes"),
        (Some((1_000, true)), "1.0 kB"),
        (Some((1_288_490_188, true)), "1.3 GB"),
        (Some((999_950, true)), "1.0 MB"),
        (Some((52_428_800, false)), "at least 52.4 MB"),
        (None, "-"),
    ];
    for (usage, expected) in cases {
        triple.data_usage = usage.map(|(bytes, complete)| {
            let mut measured = data::triple("postgresql").data_usage.expect("a size");
            measured.bytes = stackly_protocol::Count::new(bytes).expect("a count");
            measured.complete = complete;
            measured
        });
        let (root, _daemon) = serving(&triple);

        let text = run(root.path(), &["instance", "inspect", "orders-db"]);

        let line = text
            .stdout
            .lines()
            .find(|line| line.starts_with("Data size"))
            .unwrap_or_else(|| panic!("a Data size row: {text:?}"));
        assert_eq!(
            line.split_once("  ").map(|(_, v)| v.trim()),
            Some(expected),
            "{line}"
        );
    }
}

// covers: AC-7
#[test]
fn size_asks_for_a_fresh_scan_and_without_it_the_cache_only() {
    let triple = data::triple("postgresql");
    let (root, daemon) = serving(&triple);

    let cached = run(root.path(), &["instance", "inspect", "orders-db"]);
    let fresh = run(root.path(), &["instance", "inspect", "orders-db", "--size"]);

    assert_eq!(cached.code, Some(0), "{cached:?}");
    assert_eq!(fresh.code, Some(0), "{fresh:?}");
    let usage: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "instance.get_data_usage")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect();
    assert_eq!(usage.len(), 2);
    assert_eq!(
        (usage[0]["refresh"].clone(), usage[0]["cachedOnly"].clone()),
        (false.into(), true.into())
    );
    assert_eq!(
        (usage[1]["refresh"].clone(), usage[1]["cachedOnly"].clone()),
        (true.into(), false.into())
    );
}

// covers: AC-7
#[test]
fn a_data_usage_that_fails_does_not_fail_inspect() {
    let triple = data::triple("postgresql");
    let root = TestRoot::new().expect("a test root is made");
    let instance = triple.instance.clone();
    let detail_dto = triple.detail.clone();
    let _daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
            items: vec![instance.clone()],
            next: None,
        })))],
        Call::InstanceGet(_) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
        Call::InstanceGetDetail(_) => vec![Step::Reply(Reply::InstanceGetDetail(Box::new(
            detail_dto.clone(),
        )))],
        Call::InstanceGetDataUsage(_) => vec![Step::Fail(error(
            "instance.data_usage_unavailable",
            "unavailable",
            "the data folder size can't be measured",
        ))],
        _ => Vec::new(),
    });

    let text = run(root.path(), &["instance", "inspect", "orders-db", "--size"]);
    let json = run(
        root.path(),
        &["instance", "inspect", "orders-db", "--size", "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    let line = text
        .stdout
        .lines()
        .find(|line| line.starts_with("Data size"))
        .expect("a Data size row");
    assert!(line.ends_with("  unavailable"), "{line}");
    assert!(envelope(&json)["result"]["dataUsage"].is_null());
}

// covers: AC-7
#[test]
fn a_detail_of_another_revision_makes_inspect_read_the_instance_and_the_detail_once_more() {
    let triple = data::triple("postgresql");
    let root = TestRoot::new().expect("a test root is made");
    let instance = triple.instance.clone();
    let mut newer = triple.detail.clone();
    let mut older = triple.detail.clone();
    older.revision = stackly_protocol::Revision::new(instance.revision.get() - 1).expect("older");
    newer.revision = instance.revision;
    let daemon = ScriptedDaemon::serve(&root, move |call, ctx| match call {
        Call::InstanceList(_) => vec![Step::Reply(Reply::InstanceList(Box::new(InstancePage {
            items: vec![instance.clone()],
            next: None,
        })))],
        Call::InstanceGet(_) => vec![Step::Reply(Reply::InstanceGet(Box::new(instance.clone())))],
        Call::InstanceGetDetail(_) => {
            let detail = if ctx.nth == 0 { &older } else { &newer };
            vec![Step::Reply(Reply::InstanceGetDetail(Box::new(
                detail.clone(),
            )))]
        }
        Call::InstanceGetDataUsage(_) => vec![Step::Reply(Reply::InstanceGetDataUsage(
            stackly_protocol::instance::DataUsageReply { usage: None },
        ))],
        _ => Vec::new(),
    });

    let json = run(root.path(), &["instance", "inspect", "orders-db", "--json"]);

    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(
        daemon.methods(),
        [
            "daemon.handshake",
            "instance.list",
            "instance.get",
            "instance.get_detail",
            "instance.get",
            "instance.get_detail",
            "instance.get_data_usage"
        ]
    );
    assert_eq!(
        envelope(&json)["result"]["detail"]["revision"],
        triple.instance.revision.get()
    );
}
