//! AC-16: `env validate` calls `env.validate` with an absolute path,
//! prints the ok line for a valid manifest and exits 0, and for one that
//! is not valid writes the finding lines and the count to stderr and exits
//! 2 with `cli.env_invalid`, keeping the validation as result.

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::ScriptedDaemon;
use crate::common::{detail, envelope, golden, run, run_command, stackly};
use crate::data;

fn daemon(validation: stackly_protocol::env::EnvValidation) -> (TestRoot, ScriptedDaemon) {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(
        &root,
        data::serving(data::list(), validation, data::applicable(), data::detail()),
    );
    (root, daemon)
}

/// AC-16.
#[test]
fn a_valid_manifest_prints_its_ok_line_and_exits_0() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env validate over the valid validation prints ok <name> <n> services sha256:<digest> on stdout, nothing on stderr, exits 0, and writes the EnvValidation as result with --json",
    );
    let (root, _daemon) = daemon(data::valid());

    let text = run(root.path(), &["env", "validate"]);
    let json = run(root.path(), &["env", "validate", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(text.stderr, "");
    golden::check("env_validate.txt", &text.stdout);
    assert_eq!(json.code, Some(0), "{json:?}");
    let value = envelope(&json);
    assert_eq!(value["command"], "env validate");
    assert_eq!(value["result"]["valid"], true);
    assert_eq!(value["error"], serde_json::Value::Null);
    ev.pass();
}

/// AC-16.
#[test]
fn an_invalid_manifest_writes_its_findings_to_stderr_and_exits_2() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "env validate over validation-invalid.json writes one line per finding and 2 error(s), 0 warning(s) to stderr, nothing to stdout, and exits 2 with cli.env_invalid, keeping the validation as result with --json",
    );
    let (root, _daemon) = daemon(data::invalid());

    let text = run(root.path(), &["env", "validate"]);
    let json = run(root.path(), &["env", "validate", "--json"]);

    assert_eq!(text.code, Some(2), "{text:?}");
    assert_eq!(text.stdout, "");
    golden::check("env_validate_invalid_stderr.txt", &text.stderr);
    assert_eq!(json.code, Some(2), "{json:?}");
    golden::check("env_validate_invalid.json", &json.stdout);
    let value = envelope(&json);
    assert_eq!(value["error"]["code"], "cli.env_invalid");
    assert_eq!(detail(&value["error"], "errors").as_deref(), Some("2"));
    assert_eq!(detail(&value["error"], "warnings").as_deref(), Some("0"));
    assert_eq!(value["result"]["valid"], false);
    assert_eq!(value["result"]["findings"][0]["code"], "dependency_cycle");
    assert_eq!(json.stderr, "");
    ev.pass();
}

/// AC-16: `--file` defaults to `./stackly.yaml`, and a relative path is
/// made absolute and normalized before it is sent.
#[test]
fn the_path_sent_is_absolute_and_normalized_with_stackly_yaml_by_default() {
    let (root, daemon) = daemon(data::valid());
    let work = root.path().join("work");
    std::fs::create_dir_all(work.join("a")).unwrap();
    let work = work.canonicalize().unwrap();

    let mut command = stackly(root.path());
    command.current_dir(&work).args(["env", "validate"]);
    assert_eq!(run_command(command).code, Some(0));
    let mut command = stackly(root.path());
    command
        .current_dir(&work)
        .args(["env", "validate", "--file", "./a/../b/./stackly.yaml"]);
    assert_eq!(run_command(command).code, Some(0));

    let paths: Vec<String> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "env.validate")
        .map(|logged| {
            logged.raw["call"]["content"]["path"]
                .as_str()
                .expect("a path")
                .to_owned()
        })
        .collect();
    assert_eq!(
        paths,
        [
            work.join("stackly.yaml").display().to_string(),
            work.join("b/stackly.yaml").display().to_string(),
        ]
    );
}
