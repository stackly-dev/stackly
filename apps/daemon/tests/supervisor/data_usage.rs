//! `instance.get_data_usage` through the real daemon (spec 0021 AC-5,
//! AC-19): the size of a real data folder, the cache, the refusals, and the
//! folders a scan must not follow.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_protocol::instance::GetDataUsageParams;
use stackly_protocol::{Call, ErrorBody};
use stackly_testkit::allocate_port;
use stacklyd::detail::DataUsageConfig;
use uuid::Uuid;

use crate::support::{Api, Site, fast};

/// Creates `name` with a fixed port.
async fn create(api: &Api, name: &str) -> Uuid {
    let mut params = api.params(name, json!({}));
    params.endpoints = vec![Api::http_port(allocate_port().expect("a free port"))];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    id
}

fn reason(error: &ErrorBody) -> Option<&str> {
    error
        .details()
        .iter()
        .find(|detail| detail.key() == "reason")
        .map(|detail| detail.value())
}

fn data_of(site: &Site, id: Uuid) -> std::path::PathBuf {
    site.instance_paths(id).data
}

/// `files` empty files in `files / 1000` folders below `root`.
fn fill(root: &Path, files: usize) {
    for folder in 0..files.div_ceil(1000) {
        let dir = root.join(format!("d{folder}"));
        fs::create_dir(&dir).expect("a folder");
        for file in 0..1000.min(files - folder * 1000) {
            fs::File::create(dir.join(format!("f{file}"))).expect("a file");
        }
    }
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn the_size_of_a_data_folder_is_cached_and_refreshed_on_request() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "orders").await;

    assert_eq!(api.data_usage(id, false, true).await.expect("reply"), None);
    let first = api
        .data_usage(id, false, false)
        .await
        .expect("a scan")
        .expect("a size");
    assert!(first.complete);
    assert!(first.bytes.get() > 0, "the data folder holds files");
    let again = api
        .data_usage(id, false, false)
        .await
        .expect("cached")
        .expect("a size");
    assert_eq!(again, first, "the stored result comes back as it was");
    let cached = api
        .data_usage(id, false, true)
        .await
        .expect("cached")
        .expect("a size");
    assert_eq!(cached, first);
    tokio::time::sleep(Duration::from_millis(5)).await;
    let refreshed = api
        .data_usage(id, true, false)
        .await
        .expect("a scan")
        .expect("a size");
    assert!(refreshed.scanned_at > first.scanned_at);
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn refresh_together_with_cached_only_is_refused_before_any_work() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "orders").await;

    let error = api
        .client
        .call(Call::InstanceGetDataUsage(GetDataUsageParams {
            instance_id: id,
            refresh: true,
            cached_only: true,
        }))
        .await
        .expect_err("both flags together");

    assert_eq!(error.code(), "request.invalid_params");
    assert!(
        error
            .fields()
            .iter()
            .any(|field| field.path() == "cachedOnly" && field.code() == "invalid_value")
    );
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn an_instance_that_does_not_exist_is_not_found() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;

    let error = api
        .data_usage(Uuid::now_v7(), false, false)
        .await
        .expect_err("no such instance");

    assert_eq!(error.code(), "resource.not_found");
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn a_missing_a_shared_and_a_swapped_folder_each_say_why_they_cannot_be_measured() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let gone = create(&api, "gone").await;
    let open = create(&api, "open").await;
    let swapped = create(&api, "swapped").await;
    fs::remove_dir_all(data_of(&site, gone)).expect("the data folder goes");
    fs::set_permissions(data_of(&site, open), fs::Permissions::from_mode(0o755))
        .expect("the mode changes");
    let elsewhere = site.root.path().join("elsewhere");
    fs::create_dir(&elsewhere).expect("a folder");
    fs::remove_dir_all(data_of(&site, swapped)).expect("the data folder goes");
    symlink(&elsewhere, data_of(&site, swapped)).expect("a symlink");

    let missing = api
        .data_usage(gone, false, false)
        .await
        .expect_err("missing");
    let shared = api
        .data_usage(open, false, false)
        .await
        .expect_err("not private");
    let link = api
        .data_usage(swapped, false, false)
        .await
        .expect_err("a symlink");

    for (error, expected) in [
        (&missing, "missing"),
        (&shared, "not_private"),
        (&link, "not_private"),
    ] {
        assert_eq!(error.code(), "instance.data_usage_unavailable");
        assert_eq!(reason(error), Some(expected));
        assert!(
            error
                .hint()
                .is_some_and(|hint| hint.contains("stackly doctor"))
        );
    }
    assert_eq!(
        api.data_usage(gone, false, true)
            .await
            .expect("cached only"),
        None,
        "a failure is never cached"
    );
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_inside_the_data_folder_that_cannot_be_read_fails_the_scan_as_not_private() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "locked").await;
    let locked = data_of(&site, id).join("locked");
    fs::create_dir(&locked).expect("a folder");
    fs::write(locked.join("file"), b"x").expect("a file");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("the mode changes");

    let result = api.data_usage(id, false, false).await;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).expect("the mode is restored");

    let error = result.expect_err("a folder without read access");
    assert_eq!(reason(&error), Some("not_private"));
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn a_scan_counts_blocks_once_per_hard_link_and_follows_no_symlink() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "links").await;
    let data = data_of(&site, id);
    let before = api
        .data_usage(id, true, false)
        .await
        .expect("a scan")
        .expect("a size")
        .bytes
        .get();
    let outside = site.root.path().join("outside");
    fs::create_dir(&outside).expect("a folder");
    fs::write(outside.join("big"), vec![7_u8; 4 * 1024 * 1024]).expect("a big file");
    symlink(&outside, data.join("escape")).expect("a symlink out of the root");
    fs::write(data.join("one"), vec![1_u8; 1024 * 1024]).expect("a file");
    fs::hard_link(data.join("one"), data.join("two")).expect("a hard link");
    let sparse = fs::File::create(data.join("sparse")).expect("a file");
    sparse.set_len(1 << 30).expect("a sparse file");

    let after = api
        .data_usage(id, true, false)
        .await
        .expect("a scan")
        .expect("a size");

    let grown = after.bytes.get() - before;
    assert!(after.complete);
    assert!(grown >= 1024 * 1024, "the hard linked file counts: {grown}");
    assert!(
        grown < 2 * 1024 * 1024 + 64 * 1024,
        "once for two names, nothing from outside, little for a sparse file: {grown}"
    );
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_nested_deeper_than_thirty_two_levels_is_incomplete() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "deep").await;
    let mut deep = data_of(&site, id);
    for _ in 0..40 {
        deep.push("d");
    }
    fs::create_dir_all(&deep).expect("a deep tree");

    let usage = api
        .data_usage(id, true, false)
        .await
        .expect("a scan")
        .expect("a size");

    assert!(!usage.complete);
}

// covers: AC-5, AC-19
#[tokio::test(flavor = "multi_thread")]
async fn a_tree_of_250_000_entries_ends_incomplete_within_the_deadline() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let id = create(&api, "huge").await;
    fill(&data_of(&site, id), 250_000);

    let started = Instant::now();
    let usage = api
        .data_usage(id, false, false)
        .await
        .expect("a scan")
        .expect("a size");
    let took = started.elapsed();

    assert!(!usage.complete, "250 000 entries are over the limit");
    assert!(
        took < Duration::from_millis(2_100),
        "the scan ends at the deadline plus 100 ms: {took:?}"
    );
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn the_limits_of_a_scan_come_from_the_config_of_the_daemon() {
    let site = Site::new();
    let mut config = fast();
    let mut small = DataUsageConfig::default();
    small.max_entries = 3;
    config.data_usage = small;
    let _daemon = site.start_with(config).await;
    let api = site.api().await;
    let id = create(&api, "capped").await;
    fill(&data_of(&site, id), 10);

    let usage = api
        .data_usage(id, false, false)
        .await
        .expect("a scan")
        .expect("a size");

    assert!(!usage.complete);
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn three_instances_asked_at_once_are_all_measured() {
    let site = Site::new();
    let _daemon = site.start().await;
    let api = site.api().await;
    let mut ids = Vec::new();
    for n in 0..3 {
        ids.push(create(&api, &format!("db-{n}")).await);
    }

    let (a, b, c) = tokio::join!(
        api.data_usage(ids[0], false, false),
        api.data_usage(ids[1], false, false),
        api.data_usage(ids[2], false, false),
    );

    for result in [a, b, c] {
        assert!(result.expect("a scan").is_some());
    }
}
