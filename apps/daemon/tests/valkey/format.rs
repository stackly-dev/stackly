//! Text settings reach Valkey through the `valkeyConf` format and are read
//! back unchanged by the real server (spec 0040 AC-2, the round trip), and
//! the edge values of the keyspace flags setting reach Valkey as the same
//! flags a live `CONFIG SET` of that value gives.

use serde_json::json;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    Site, Valkey, copy_tree, create_instance, package_dir, port_of, stop_and_shutdown,
};

/// The text of AC-2: a quote, a backslash, a space, Unicode, and `#`.
const ODD_TEXT: &str = "a\"b\\c d ü #x";

/// A copy of the package whose `valkey.conf` sets `proc-title-template`
/// from a free text setting, so the test can send any text through the
/// real escaper; `set-proc-title no` keeps it from changing the process.
fn copy_with_free_text(root: &TestRoot) -> std::path::PathBuf {
    let dir = root.path().join("valkey");
    copy_tree(&package_dir(), &dir);
    let template = dir.join("templates/valkey.conf.tmpl");
    let mut text = std::fs::read_to_string(&template).expect("the template can be read");
    text.push_str("proc-title-template {{ settings.procTitle }}\n");
    std::fs::write(&template, text).expect("the template can be written");
    let schema_path = dir.join("schemas/settings.schema.json");
    let mut schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path).expect("the schema can be read"),
    )
    .expect("the schema is JSON");
    schema["properties"]["procTitle"] = json!({
        "type": "string",
        "title": "Process title",
        "maxLength": 100,
        "default": "{title}",
        "x-stackly": { "apply": "restart", "visibility": "advanced" }
    });
    std::fs::write(
        &schema_path,
        serde_json::to_string_pretty(&schema).expect("the schema serializes"),
    )
    .expect("the schema can be written");
    dir
}

// covers: AC-2 (spec 0040)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_reads_back_the_text_the_valkey_conf_escaper_wrote() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "CONFIG GET proc-title-template on Valkey 9.1.2 reads back a\"b\\c d ü #x exactly as it was given to the valkeyConf format",
    );
    let copies = TestRoot::new().expect("a folder for the package copy");
    let dir = copy_with_free_text(&copies);
    let Some(site) = Site::open_package(&ev, &dir) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "quoted", json!({ "procTitle": ODD_TEXT })).await;
    let running = api.start_ready(made.id).await;
    let mut valkey = Valkey::signed_in(port_of(&running), &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);
    let read = valkey.cmd(&["CONFIG", "GET", "proc-title-template"]);
    assert_eq!(read.config_value(), Some(ODD_TEXT), "{read:?}");
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!("{ODD_TEXT} came back unchanged from CONFIG GET"));
}

// covers: AC-2 (spec 0040)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_takes_keyspace_flags_with_a_dollar_sign() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "a Valkey instance with notifyKeyspaceEvents K$ starts, and CONFIG GET notify-keyspace-events holds K and $",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(
        &site,
        &api,
        "flags",
        json!({ "notifyKeyspaceEvents": "K$" }),
    )
    .await;
    let running = api.start_ready(made.id).await;
    let mut valkey = Valkey::signed_in(port_of(&running), &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);
    let read = valkey.cmd(&["CONFIG", "GET", "notify-keyspace-events"]);
    let value = read.config_value().unwrap_or_default().to_owned();
    assert!(value.contains('K') && value.contains('$'), "{read:?}");
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!("notify-keyspace-events is {value}"));
}

/// Edge values of `notifyKeyspaceEvents` the anchored schema pattern
/// accepts: empty, `$` with and without a type letter, `A` (which Valkey
/// expands), every letter once, and repeats.
const KEYSPACE_EDGES: &[&str] = &[
    "",
    "$",
    "K$",
    "E$x",
    "KEA",
    "AKE$",
    "KEg$lshzxetdmnA",
    "KKEE",
];

// covers: AC-2 (spec 0040)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_reads_back_each_edge_keyspace_value_as_a_live_config_set_of_it() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "for each edge value of notifyKeyspaceEvents (empty, $, K$, E$x, KEA, AKE$, every letter, repeats) the instance starts, and CONFIG GET notify-keyspace-events reads the same flags that CONFIG SET of that value on the running server reads",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let mut version = String::new();
    let mut seen = Vec::new();
    for (index, value) in KEYSPACE_EDGES.iter().enumerate() {
        let made = create_instance(
            &site,
            &api,
            &format!("flags{index}"),
            json!({ "notifyKeyspaceEvents": value }),
        )
        .await;
        let running = api.start_ready(made.id).await;
        let mut valkey = Valkey::signed_in(port_of(&running), &made.password);
        if version.is_empty() {
            version = valkey.info_server("valkey_version");
            site.subject(&ev, &version);
        }
        let rendered = valkey
            .cmd(&["CONFIG", "GET", "notify-keyspace-events"])
            .config_value()
            .unwrap_or_else(|| panic!("{value:?}: CONFIG GET answers"))
            .to_owned();
        let set = valkey.cmd(&["CONFIG", "SET", "notify-keyspace-events", value]);
        assert_eq!(
            set,
            crate::harness::Reply::Simple("OK".to_owned()),
            "{value:?}"
        );
        let live = valkey
            .cmd(&["CONFIG", "GET", "notify-keyspace-events"])
            .config_value()
            .unwrap_or_else(|| panic!("{value:?}: CONFIG GET answers"))
            .to_owned();
        assert_eq!(rendered, live, "{value:?} through the config file");
        seen.push(format!("{value:?}->{rendered:?}"));
        drop(valkey);
        crate::harness::stop_instance(&api, made.id).await;
    }
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "Valkey {version} read back {} as a live CONFIG SET does",
        seen.join(", ")
    ));
}
