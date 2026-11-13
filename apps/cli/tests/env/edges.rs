//! AC-16, AC-17 at the edges: `env remove` asks y/N on a terminal and
//! sends nothing until yes, and `env validate` counts every finding when
//! the daemon sent only the first hundred.

use stackly_protocol::Count;
use stackly_protocol::env::{EnvFinding, EnvValidation};
use stackly_testkit::TestRoot;

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::ScriptedDaemon;
use crate::common::{detail, envelope, run};
use crate::data;

fn removes(daemon: &ScriptedDaemon) -> usize {
    daemon
        .methods()
        .iter()
        .filter(|method| *method == "env.remove")
        .count()
}

/// AC-17: on a terminal `env remove` asks with the `ConfirmArgs` rule; no
/// is `cli.confirmation_declined` with nothing sent, yes removes once.
#[test]
fn remove_on_a_terminal_asks_and_sends_nothing_until_yes() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::serving(
            data::list(),
            data::valid(),
            data::applicable(),
            data::detail(),
        ),
    );
    let typed = |input| PtyOptions {
        stdin_tty: true,
        input: Some(input),
        ..PtyOptions::default()
    };

    let declined = run_in_pty(
        root.path(),
        &["env", "remove", "erp-development"],
        &typed("n\n"),
    );

    assert_eq!(declined.code, Some(9), "{declined:?}");
    assert!(declined.stderr.contains("[y/N]"), "{}", declined.stderr);
    assert!(
        declined.stderr.contains("erp-development"),
        "{}",
        declined.stderr
    );
    assert!(
        declined.stderr.contains("cli.confirmation_declined"),
        "{}",
        declined.stderr
    );
    assert_eq!(removes(&daemon), 0, "nothing is sent on no");

    let empty = run_in_pty(
        root.path(),
        &["env", "remove", "erp-development"],
        &typed("\n"),
    );
    assert_eq!(empty.code, Some(9), "the default is no: {empty:?}");
    assert_eq!(removes(&daemon), 0);

    let accepted = run_in_pty(
        root.path(),
        &["env", "remove", "erp-development"],
        &typed("y\n"),
    );

    assert_eq!(accepted.code, Some(0), "{accepted:?}");
    assert_eq!(
        accepted.stdout,
        "Removed environment erp-development. Instances and data are kept.\n"
    );
    assert_eq!(removes(&daemon), 1);
}

/// A validation with `count` error findings of which the daemon sent the
/// first hundred.
fn many(count: u64) -> EnvValidation {
    let mut validation = data::invalid();
    let template: EnvFinding = validation.findings[0].clone();
    validation.findings = (0..100)
        .map(|n| {
            let mut finding = template.clone();
            finding.location = format!("services.s{n}.endpoints.a");
            finding
        })
        .collect();
    validation.finding_count = Count::saturating(count);
    validation.omitted = Count::saturating(count - 100);
    validation
}

/// AC-16, AC-4: with findings omitted, the lines show the hundred that
/// came, a line says how many more there are, and the count and
/// `cli.env_invalid` hold every finding.
#[test]
fn omitted_findings_are_counted_in_the_summary_and_the_error() {
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(
        &root,
        data::serving(
            data::list(),
            many(128),
            data::invalid_plan(),
            data::detail(),
        ),
    );

    let text = run(root.path(), &["env", "validate"]);
    let json = run(root.path(), &["env", "validate", "--json"]);

    assert_eq!(text.code, Some(2), "{text:?}");
    let lines: Vec<&str> = text.stderr.lines().collect();
    let shown = lines
        .iter()
        .filter(|line| line.contains(": services.s"))
        .count();
    assert_eq!(shown, 100, "{}", text.stderr);
    assert!(
        lines.contains(&"28 more finding(s) not shown"),
        "{}",
        text.stderr
    );
    assert!(
        lines.contains(&"128 error(s), 0 warning(s)"),
        "{}",
        text.stderr
    );
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.env_invalid");
    assert_eq!(detail(&value["error"], "errors").as_deref(), Some("128"));
    assert_eq!(detail(&value["error"], "warnings").as_deref(), Some("0"));
    assert_eq!(value["result"]["omitted"], 28);
}
