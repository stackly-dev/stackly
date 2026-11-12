//! A create never uses a data folder that already holds data (spec 0042
//! AC-11): a valid MySQL 8.4 data folder that appears in the new data
//! folder before the content check fails the create with
//! `instance.data_not_empty` before init, nothing is spawned, and every file
//! of the copy keeps its bytes.

use std::sync::Arc;

use stackly_core::instance::create::Hook;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};

use crate::harness::{
    MY84, My, Site, copy_tree, create_instance, failed_create, mysqld_serving, new_password,
    tree_digest,
};
use crate::support::Api;

async fn refuses_a_create_on_a_copy_of_valid_data(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    // A valid data folder: the one init made for a first instance.
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let source = create_instance(&site, &api, "source").await;
    let copies = TestRoot::new().expect("a folder for the data copy");
    let copy = copies.path().join("data");
    copy_tree(&site.instance_paths(source.id).data, &copy);
    assert!(copy.join("mysql_upgrade_history").is_file());
    let before = tree_digest(&copy);
    drop(api);
    daemon.shutdown().await;

    let planted = copy.clone();
    let hook: Hook = Arc::new(move |paths| copy_tree(&planted, &paths.data));
    let daemon = site
        .in_process(|config| config.create_hooks.after_layout = Some(hook))
        .await;
    let api = site.api().await;
    let (id, detail) = failed_create(&site, &api, "adopt", &new_password()).await;

    assert_eq!(
        Api::failure_code(&detail),
        "instance.data_not_empty",
        "{detail:?}"
    );
    let steps: Vec<&str> = detail.steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        !steps.contains(&"init"),
        "the create reached init: {steps:?}"
    );
    let data = site.instance_paths(id).data;
    assert!(mysqld_serving(&data).is_empty(), "a mysqld was spawned");
    assert!(
        data.is_dir(),
        "the refused create removed the data folder with the planted copy"
    );
    assert_eq!(tree_digest(&data), before, "a planted file was changed");
    assert_eq!(tree_digest(&copy), before, "the copy was changed");
    daemon.shutdown().await;
    ev.subject(
        "mysql",
        my.version,
        Some(&site.artifact_sha256),
        Source::Managed,
    );
    ev.pass_with(&format!(
        "a copy of {} valid data files failed the create with instance.data_not_empty before init, nothing spawned, every file unchanged",
        before.len()
    ));
}

// covers: AC-11 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_refuses_a_create_on_a_copy_of_valid_data() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "a copy of a valid MySQL 8.4 data folder planted in the new data folder after the layout fails the create with instance.data_not_empty before init, no mysqld is spawned, and every file of the copy keeps its bytes",
    );
    refuses_a_create_on_a_copy_of_valid_data(MY84, ev).await;
}
