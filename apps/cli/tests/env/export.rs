//! Spec 0045 AC-17, AC-14: `env export` sends `env.export` with the
//! format its flag names, prints exactly the daemon's `text` to stdout and
//! one `skipped:` line per entry to stderr, and exits 0; an environment
//! that is not registered exits 10.

use stackly_protocol::env::{EnvExport, EnvSource, ExportFormat};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{envelope, golden, run};
use crate::data;

/// A daemon that knows the fixture environment and answers `env.export`
/// with `export` in the format asked, failing `env.not_registered` for a
/// file under `unregistered/`.
fn daemon(export: EnvExport) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let list = data::list();
    let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
        Call::EnvList => vec![Step::Reply(Reply::EnvList(list.clone()))],
        Call::EnvExport(params) => match &params.source {
            EnvSource::File(path) if path.contains("/unregistered/") => vec![Step::Fail(error(
                "env.not_registered",
                "not_found",
                "the environment of this file is not registered",
            ))],
            _ => {
                let mut export = export.clone();
                if params.format == ExportFormat::Shell {
                    export.format = ExportFormat::Shell;
                    export.text = export
                        .variables
                        .iter()
                        .map(|variable| format!("export {}='{}'\n", variable.name, variable.value))
                        .collect();
                }
                vec![Step::Reply(Reply::EnvExport(Box::new(export)))]
            }
        },
        _ => Vec::new(),
    });
    (root, daemon)
}

/// AC-17.
#[test]
fn export_prints_exactly_the_text_and_the_skipped_entries_on_stderr() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env export over export-dotenv.json prints exactly its text to stdout, skipped: postgres.socket: unix_endpoint and skipped: cache: not_created to stderr, exits 0, sends format dotenv by default and shell with --format shell, and with --json writes the EnvExport with an empty stderr",
    );
    let fixture = data::export_dotenv();
    let (root, daemon) = daemon(fixture.clone());

    let dotenv = run(root.path(), &["env", "export"]);
    let shell = run(root.path(), &["env", "export", "--format", "shell"]);
    let json = run(root.path(), &["env", "export", "--json"]);
    let quiet = run(root.path(), &["env", "export", "--quiet"]);

    assert_eq!(dotenv.code, Some(0), "{dotenv:?}");
    assert_eq!(dotenv.stdout, fixture.text);
    golden::check("env_export.txt", &dotenv.stdout);
    golden::check("env_export_stderr.txt", &dotenv.stderr);
    assert_eq!(shell.code, Some(0), "{shell:?}");
    assert_eq!(
        shell.stdout.lines().next(),
        Some("export STACKLY_POSTGRES_CLIENT_HOST='127.0.0.1'")
    );
    assert_eq!(json.code, Some(0), "{json:?}");
    assert_eq!(json.stderr, "");
    let value = envelope(&json);
    assert_eq!(value["command"], "env export");
    assert_eq!(value["result"]["text"], fixture.text);
    assert_eq!(quiet.stdout, fixture.text);
    assert_eq!(quiet.stderr, "");
    let formats: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.export")
        .map(|logged| logged.raw["call"]["content"]["format"].clone())
        .collect();
    assert_eq!(formats, ["dotenv", "shell", "dotenv", "dotenv"]);
    ev.pass();
}

/// AC-17: every variable skipped still exits 0 with an empty stdout.
#[test]
fn an_export_with_every_service_skipped_exits_0_with_nothing_on_stdout() {
    let mut export = data::export_dotenv();
    export.text.clear();
    export.variables.clear();
    let (root, _daemon) = daemon(export);

    let text = run(root.path(), &["env", "export"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stdout, "");
    assert!(
        text.stderr.contains("skipped: cache: not_created"),
        "{}",
        text.stderr
    );
}

/// AC-17: a name that is not registered and a file whose environment is
/// not registered both exit 10; a name exports that environment by ID.
#[test]
fn an_environment_that_is_not_registered_exits_10() {
    let (root, daemon) = daemon(data::export_dotenv());
    let id = data::detail().summary.id.to_string();
    let file = root.path().join("unregistered/stackly.yaml");
    let file = file.to_str().expect("the path is utf-8");

    let named = run(root.path(), &["env", "export", "erp-development"]);
    let unknown = run(root.path(), &["env", "export", "nope", "--json"]);
    let unregistered = run(root.path(), &["env", "export", "--file", file, "--json"]);

    assert_eq!(named.code, Some(0), "{named:?}");
    assert_eq!(unknown.code, Some(10), "{unknown:?}");
    assert_eq!(
        envelope(&unknown)["error"]["code"],
        "cli.environment_not_found"
    );
    assert_eq!(unregistered.code, Some(10), "{unregistered:?}");
    assert_eq!(
        envelope(&unregistered)["error"]["code"],
        "env.not_registered"
    );
    let sources: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.export")
        .map(|logged| logged.raw["call"]["content"]["source"].clone())
        .collect();
    assert_eq!(
        sources[0],
        serde_json::json!({ "type": "registered", "content": id })
    );
}
