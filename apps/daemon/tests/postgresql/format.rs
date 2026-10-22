//! A text setting reaches `postgresql.conf` and is read back unchanged by
//! the real server (spec 0019 AC-2, the round trip).

use std::path::Path;
use std::process::Command;

use serde_json::json;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_with, new_password, package_dir};

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a folder can be made");
    for entry in std::fs::read_dir(from)
        .expect("a folder can be read")
        .flatten()
    {
        let target = to.join(entry.file_name());
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a file can be copied");
        }
    }
}

/// A copy of the package whose `log_line_prefix` is a text setting, the only
/// free text parameter a person could set, so the test can send any text
/// through the real `postgresqlConf` escaper.
fn copy_with_free_text(root: &TestRoot) -> std::path::PathBuf {
    let dir = root.path().join("postgresql");
    copy_tree(&package_dir(), &dir);
    let template = dir.join("templates/postgresql.conf.tmpl");
    let text = std::fs::read_to_string(&template).expect("the template can be read");
    let changed = text.replace(
        "log_line_prefix = '%m [%p] %q%u@%d '",
        "log_line_prefix = {{ settings.logLinePrefix }}",
    );
    assert_ne!(text, changed, "the template has the log_line_prefix line");
    std::fs::write(&template, changed).expect("the template can be written");
    let schema_path = dir.join("schemas/settings.schema.json");
    let mut schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path).expect("the schema can be read"),
    )
    .expect("the schema is JSON");
    schema["properties"]["logLinePrefix"] = json!({
        "type": "string",
        "title": "Log line prefix",
        "maxLength": 100,
        "default": "%m [%p] %q%u@%d ",
        "x-stackly": { "apply": "restart", "visibility": "advanced" }
    });
    std::fs::write(
        &schema_path,
        serde_json::to_string_pretty(&schema).expect("the schema serializes"),
    )
    .expect("the schema can be written");
    dir
}

async fn reads_back_the_text_the_postgresql_conf_escaper_wrote(pg: Pg) {
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = &format!(
            "postgres -C log_line_prefix from runtime {} reads back a'b\\c d and ü 空 exactly as they were given to the postgresqlConf format",
            pg.major
        ),
    );
    let copies = TestRoot::new().expect("a folder for the package copy");
    let dir = copy_with_free_text(&copies);
    let Some(site) = Site::open_package(&ev, pg, &dir) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    for (name, value) in [("quoted", "a'b\\c d"), ("unicode", "ü 空")] {
        let made = create_with(
            &site,
            &api,
            name,
            &new_password(),
            json!({ "logLinePrefix": value }),
        )
        .await;
        let paths = site.instance_paths(made.id);
        let config = std::fs::read_to_string(paths.config.join("postgresql.conf"))
            .expect("the config was rendered by the init");
        println!(
            "{}",
            config
                .lines()
                .find(|l| l.starts_with("log_line_prefix"))
                .unwrap_or("")
        );
        let output = Command::new(site.bin("postgres"))
            .arg("-D")
            .arg(&paths.data)
            .arg(format!(
                "--config-file={}",
                paths.config.join("postgresql.conf").display()
            ))
            .args(["-C", "log_line_prefix"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("postgres runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let read = String::from_utf8_lossy(&output.stdout);
        assert_eq!(read.trim_end_matches('\n'), value, "{name}");
    }
    daemon.shutdown().await;
    site.subject(&ev, pg.version);
    ev.pass_with("a'b\\c d and ü 空 came back unchanged from postgres -C log_line_prefix");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_reads_back_the_text_the_postgresql_conf_escaper_wrote() {
    reads_back_the_text_the_postgresql_conf_escaper_wrote(PG17).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_reads_back_the_text_the_postgresql_conf_escaper_wrote() {
    reads_back_the_text_the_postgresql_conf_escaper_wrote(PG18).await;
}
