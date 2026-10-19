//! AC-1: the exit code comes from the error category alone.

use stackly_protocol::Reply;
use stackly_protocol::daemon::DaemonHealthStatus;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{dto, envelope, run};

/// Every category with its exit code (spec 0008, *Peta exit code*).
const TABLE: [(&str, i32); 10] = [
    ("internal", 1),
    ("invalid_input", 2),
    ("conflict", 3),
    ("unauthorized", 4),
    ("unsupported", 5),
    ("unavailable", 6),
    ("failed", 7),
    ("timeout", 8),
    ("cancelled", 9),
    ("not_found", 10),
];

#[test]
fn every_category_exits_with_its_code_in_text_and_json_whatever_the_code() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "for every error category, a daemon answer with a code of that category, synthetic codes the cli does not know included, makes stackly daemon status exit with the code of the category in text and in --json",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = ScriptedDaemon::serve(&root, |call, ctx| {
        if call.method() != "daemon.get_info" {
            return vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
                DaemonHealthStatus::Ready,
            )))];
        }
        let (category, _) = TABLE[(ctx.nth / 2) % TABLE.len()];
        vec![Step::Fail(error(
            &format!("test.{category}"),
            category,
            "a synthetic code",
        ))]
    });

    for (category, code) in TABLE {
        let text = run(root.path(), &["daemon", "status"]);
        let json = run(root.path(), &["daemon", "status", "--json"]);

        assert_eq!(text.code, Some(code), "{category}: {text:?}");
        assert!(
            text.stderr.contains(&format!("code: test.{category}")),
            "{category}: {}",
            text.stderr
        );
        assert_eq!(text.stdout, "", "{category}");
        assert_eq!(json.code, Some(code), "{category}: {json:?}");
        let value = envelope(&json);
        assert_eq!(value["error"]["code"], format!("test.{category}"));
        assert_eq!(value["error"]["category"], category);
    }
    ev.pass();
}

#[test]
fn the_message_and_the_code_never_change_the_exit_code() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "two codes of one category with different messages exit alike, and one message under two categories exits apart",
    );
    let root = TestRoot::new().unwrap();
    let answers = [
        error("registry.busy", "unavailable", "the registry is busy"),
        error("test.other", "unavailable", "something else entirely"),
        error("test.same", "conflict", "one message"),
        error("test.same", "failed", "one message"),
    ];
    let _daemon = ScriptedDaemon::serve(&root, move |_, ctx| {
        vec![Step::Fail(answers[ctx.nth % answers.len()].clone())]
    });

    let codes: Vec<Option<i32>> = (0..4)
        .map(|_| run(root.path(), &["daemon", "status", "--json"]).code)
        .collect();

    assert_eq!(codes, [Some(6), Some(6), Some(3), Some(7)]);
    ev.pass();
}

#[test]
fn help_and_version_are_text_on_stdout_with_exit_0_even_with_json() {
    let root = TestRoot::new().unwrap();
    for args in [
        &["--help"][..],
        &["--json", "--help"],
        &["daemon", "status", "--help", "--json"],
        &["help", "daemon", "status"],
        &["daemon", "--help"],
    ] {
        let help = run(root.path(), args);
        assert_eq!(help.code, Some(0), "{args:?}: {help:?}");
        assert!(help.stdout.contains("Usage: stackly"), "{args:?}: {help:?}");
        assert!(!help.stdout.starts_with('{'), "{args:?}");
        assert_eq!(help.stderr, "", "{args:?}");
    }
    for args in [&["--version"][..], &["--json", "--version"]] {
        let version = run(root.path(), args);
        assert_eq!(version.code, Some(0));
        let protocol = stackly_protocol::ProtocolVersion::CURRENT;
        assert_eq!(
            version.stdout,
            format!(
                "stackly {} (protocol {}.{})\n",
                env!("CARGO_PKG_VERSION"),
                protocol.major,
                protocol.minor
            )
        );
    }
    assert!(
        std::fs::read_dir(root.path()).unwrap().next().is_none(),
        "help and version touch nothing"
    );
}

#[test]
fn refused_arguments_exit_2_as_text_or_as_a_cli_usage_envelope_with_json() {
    let ev = evidence!(
        covers = ["REQ-028", "AC-028"],
        kind = Kind::Fixture,
        expected = "arguments clap refuses exit 2: as clap text on stderr, or with --json before -- as one cli.usage envelope with an empty command and the error kind",
    );
    let root = TestRoot::new().unwrap();

    let text = run(root.path(), &["daemon", "status", "--nope"]);
    assert_eq!(text.code, Some(2));
    assert_eq!(text.stdout, "");
    assert!(
        text.stderr
            .starts_with("error: unexpected argument '--nope'")
    );

    for (args, kind) in [
        (
            &["daemon", "status", "--nope", "--json"][..],
            "unknown_argument",
        ),
        (&["--json", "daemon", "nope"], "invalid_subcommand"),
        (
            &["--json", "--color", "pink", "daemon", "status"],
            "invalid_value",
        ),
        (&["--json"], "missing_subcommand"),
        (&["daemon", "--json"], "missing_subcommand"),
    ] {
        let refused = run(root.path(), args);
        assert_eq!(refused.code, Some(2), "{args:?}");
        let value = envelope(&refused);
        assert_eq!(value["command"], "", "{args:?}");
        assert_eq!(value["result"], serde_json::Value::Null);
        assert_eq!(value["error"]["code"], "cli.usage", "{args:?}");
        assert_eq!(value["error"]["category"], "invalid_input");
        assert_eq!(
            crate::common::detail(&value["error"], "kind").as_deref(),
            Some(kind),
            "{args:?}"
        );
        let message = value["error"]["message"].as_str().unwrap();
        assert!(
            !message.starts_with("error:") && !message.ends_with('.'),
            "{message}"
        );
        assert_eq!(refused.stderr, "", "{args:?}");
    }

    let after_dash = run(root.path(), &["daemon", "status", "--", "--json"]);
    assert_eq!(after_dash.code, Some(2));
    assert_eq!(after_dash.stdout, "", "--json after -- does not count");
    assert!(after_dash.stderr.starts_with("error: "));

    let bare = run(root.path(), &[]);
    assert_eq!(bare.code, Some(2));
    assert_eq!(bare.stdout, "");
    assert!(bare.stderr.contains("Usage: stackly"), "{bare:?}");
    ev.pass();
}
