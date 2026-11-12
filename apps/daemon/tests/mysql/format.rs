//! A text setting reaches `my.cnf` through the `mysqlCnf` escaper and the
//! option file reader of the real runtime reads it back unchanged (spec 0042
//! AC-2, the round trip).

use std::process::Command;

use serde_json::json;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{
    MY84, MY97, My, Site, copy_tree, create_with, new_password, package_dir, stop_and_shutdown,
    variable,
};

/// The text of AC-2: a double quote, a backslash, a space, `#`, Unicode, a
/// single quote, and `;`.
const NASTY: &str = "a\"b\\c d # e ü 空 'q' ; z";

/// A copy of the package whose `my.cnf` takes a text setting as the value
/// of `general_log_file`, so the test can send any text through the real
/// `mysqlCnf` escaper. The argv still names the general log file, so the
/// server ignores the value.
fn copy_with_free_text(root: &TestRoot) -> std::path::PathBuf {
    let dir = root.path().join("mysql");
    copy_tree(&package_dir(), &dir);
    let template = dir.join("templates/my.cnf.tmpl");
    let text = std::fs::read_to_string(&template).expect("the template can be read");
    let changed = text.replace(
        "default_time_zone = \"+00:00\"\n",
        "default_time_zone = \"+00:00\"\ngeneral_log_file = {{ settings.generalLogFile }}\n",
    );
    assert_ne!(text, changed, "the template has the time zone line");
    std::fs::write(&template, changed).expect("the template can be written");
    let schema_path = dir.join("schemas/settings.schema.json");
    let mut schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path).expect("the schema can be read"),
    )
    .expect("the schema is JSON");
    schema["properties"]["generalLogFile"] = json!({
        "type": "string",
        "title": "General log file",
        "maxLength": 200,
        "default": "general.log",
        "x-stackly": { "apply": "restart", "visibility": "advanced" }
    });
    std::fs::write(
        &schema_path,
        serde_json::to_string_pretty(&schema).expect("the schema serializes"),
    )
    .expect("the schema can be written");
    dir
}

async fn reads_back_a_text_value_exactly_as_it_was_set(my: My, ev: Evidence) {
    let copies = TestRoot::new().expect("a folder for the package copy");
    let dir = copy_with_free_text(&copies);
    let Some(site) = Site::open_package(&ev, my, &dir) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_with(
        &site,
        &api,
        "format",
        &new_password(),
        json!({ "generalLogFile": NASTY }),
    )
    .await;
    // The config is rendered before the spawn, and the server starts on it.
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let my_cnf = site.instance_paths(made.id).config.join("my.cnf");
    let output = Command::new(site.bin("my_print_defaults"))
        .arg(format!("--defaults-file={}", my_cnf.display()))
        .arg("mysqld")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", site.root.path())
        .output()
        .expect("my_print_defaults runs");
    assert!(output.status.success(), "{output:?}");
    let printed = String::from_utf8(output.stdout).expect("UTF-8 output");
    let values: Vec<&str> = printed
        .lines()
        .filter_map(|line| line.strip_prefix("--general_log_file="))
        .collect();
    assert_eq!(values, [NASTY], "{printed}");
    let rendered = std::fs::read_to_string(&my_cnf).expect("my.cnf can be read");
    assert!(
        rendered.contains("general_log_file = \"a\\\"b\\\\c d # e ü 空 'q' ; z\"\n"),
        "{rendered}"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("my_print_defaults read the AC-2 text back byte for byte");
}

// covers: AC-2 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_reads_back_a_text_value_exactly_as_it_was_set() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "a text with a double quote, a backslash, spaces, #, Unicode, a single quote, and ; rendered by mysqlCnf as general_log_file under [mysqld] comes back from my_print_defaults of the runtime as --general_log_file= and exactly that text",
    );
    reads_back_a_text_value_exactly_as_it_was_set(MY84, ev).await;
}

/// Texts at the edges of the `mysqlCnf` escaper: a trailing backslash, an
/// escaped quote, the comment characters, outer spaces, a single quote
/// pair, backslash letters the option reader knows, and Unicode.
const EDGES: [&str; 16] = [
    "a\\",
    "\\",
    "\"",
    "\\\"",
    "\"\\",
    "\"\"",
    "#x",
    " # ",
    ";x",
    " a ",
    "'a\"b'",
    "\\n\\t\\s\\b",
    "\\x",
    "$HOME `id`",
    "ü 空 \u{1f600}",
    "a\\\\\"b # c ; d",
];

/// A copy of the package whose `my.cnf` takes every text of [`EDGES`] as an
/// option of the group `[stackly_edge]`, which `mysqld` never reads, and
/// two booleans as `local_infile` and `performance_schema`, whose defaults
/// are the other value.
fn copy_with_edges(root: &TestRoot) -> std::path::PathBuf {
    let dir = root.path().join("mysql");
    copy_tree(&package_dir(), &dir);
    let template = dir.join("templates/my.cnf.tmpl");
    let text = std::fs::read_to_string(&template).expect("the template can be read");
    let mut changed = text.replace(
        "default_time_zone = \"+00:00\"\n",
        "default_time_zone = \"+00:00\"\nlocal_infile = {{ settings.flagOn }}\nperformance_schema = {{ settings.flagOff }}\n",
    );
    assert_ne!(text, changed, "the template has the time zone line");
    changed.push_str("[stackly_edge]\n");
    for index in 0..EDGES.len() {
        changed.push_str(&format!("edge_{index} = {{{{ settings.edge{index} }}}}\n"));
    }
    std::fs::write(&template, changed).expect("the template can be written");
    let schema_path = dir.join("schemas/settings.schema.json");
    let mut schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path).expect("the schema can be read"),
    )
    .expect("the schema is JSON");
    for (name, default) in [("flagOn", false), ("flagOff", true)] {
        schema["properties"][name] = json!({
            "type": "boolean",
            "title": name,
            "default": default,
            "x-stackly": { "apply": "restart", "visibility": "advanced" }
        });
    }
    for index in 0..EDGES.len() {
        schema["properties"][format!("edge{index}")] = json!({
            "type": "string",
            "title": format!("Edge {index}"),
            "maxLength": 200,
            "default": "x",
            "x-stackly": { "apply": "restart", "visibility": "advanced" }
        });
    }
    std::fs::write(
        &schema_path,
        serde_json::to_string_pretty(&schema).expect("the schema serializes"),
    )
    .expect("the schema can be written");
    dir
}

async fn reads_back_every_edge_text_and_both_booleans(my: My, ev: Evidence) {
    let copies = TestRoot::new().expect("a folder for the package copy");
    let dir = copy_with_edges(&copies);
    let Some(site) = Site::open_package(&ev, my, &dir) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let mut settings = serde_json::Map::new();
    settings.insert("flagOn".to_owned(), json!(true));
    settings.insert("flagOff".to_owned(), json!(false));
    for (index, edge) in EDGES.iter().enumerate() {
        settings.insert(format!("edge{index}"), json!(edge));
    }
    let made = create_with(
        &site,
        &api,
        "edges",
        &new_password(),
        serde_json::Value::Object(settings),
    )
    .await;
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let my_cnf = site.instance_paths(made.id).config.join("my.cnf");
    let rendered = std::fs::read_to_string(&my_cnf).expect("my.cnf can be read");
    assert!(rendered.contains("local_infile = ON\n"), "{rendered}");
    assert!(
        rendered.contains("performance_schema = OFF\n"),
        "{rendered}"
    );
    assert_eq!(variable(&mysql, &made.password, "local_infile"), "1");
    assert_eq!(variable(&mysql, &made.password, "performance_schema"), "0");

    let output = Command::new(site.bin("my_print_defaults"))
        .arg(format!("--defaults-file={}", my_cnf.display()))
        .arg("stackly_edge")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", site.root.path())
        .output()
        .expect("my_print_defaults runs");
    assert!(output.status.success(), "{output:?}");
    let printed = String::from_utf8(output.stdout).expect("UTF-8 output");
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines.len(), EDGES.len(), "{printed}");
    for (index, edge) in EDGES.iter().enumerate() {
        let prefix = format!("--edge_{index}=");
        let value = lines
            .iter()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("edge_{index} is printed: {printed}"));
        assert_eq!(value, *edge, "edge_{index} in {rendered}");
    }

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "my_print_defaults read {} edge texts back byte for byte, and mysqld read ON as local_infile 1 and OFF as performance_schema 0",
        EDGES.len()
    ));
}

// covers: AC-2 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_reads_back_every_edge_text_and_both_booleans() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "texts with a trailing backslash, escaped quotes, #, ;, outer spaces, single quotes, backslash letters, and Unicode rendered by mysqlCnf come back from my_print_defaults of the MySQL 8.4 runtime byte for byte, and the booleans render as ON and OFF and read back as @@local_infile 1 and @@performance_schema 0",
    );
    reads_back_every_edge_text_and_both_booleans(MY84, ev).await;
}

// covers: AC-2 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_reads_back_every_edge_text_and_both_booleans() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "texts with a trailing backslash, escaped quotes, #, ;, outer spaces, single quotes, backslash letters, and Unicode rendered by mysqlCnf come back from my_print_defaults of the MySQL 9.7 runtime byte for byte, and the booleans render as ON and OFF and read back as @@local_infile 1 and @@performance_schema 0",
    );
    reads_back_every_edge_text_and_both_booleans(MY97, ev).await;
}
