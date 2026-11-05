//! `storage.set_location` and `storage.list_locations` through the real
//! daemon on the real filesystem (spec 0032 AC-19, AC-20): a folder becomes
//! the default for what is created next and moves nothing, every rule is
//! refused with its own code, a folder that went away stops a start, and
//! the preflight says so before a create.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use serde_json::json;
use stackly_ipc::CallOptions;
use stackly_protocol::instance::{PreflightCheckKind, PreflightStatus};
use stackly_protocol::settings::{
    LocationKind, LocationList, LocationTarget, SetLocationParams, StorageLocationView,
};
use stackly_protocol::{Call, ErrorBody, Reply, Revision};

use super::settings::{settings_of, stop_daemon};
use crate::support::{Api, Site, fast, fast_with_catalog, key};

async fn set_location(
    api: &Api,
    kind: LocationKind,
    target: LocationTarget,
    key: &str,
) -> Result<StorageLocationView, Box<ErrorBody>> {
    let revision = settings_of(api).await.revision;
    set_location_at(api, kind, target, revision, key).await
}

async fn set_location_at(
    api: &Api,
    kind: LocationKind,
    target: LocationTarget,
    expected: Revision,
    key: &str,
) -> Result<StorageLocationView, Box<ErrorBody>> {
    match api
        .client
        .call_detailed(
            Call::StorageSetLocation(SetLocationParams {
                kind,
                target,
                expected_revision: expected,
            }),
            CallOptions::default().with_idempotency_key(key),
        )
        .await?
    {
        Reply::StorageSetLocation(view) => Ok(*view),
        other => panic!("not a set_location reply: {other:?}"),
    }
}

async fn locations(api: &Api) -> LocationList {
    match api
        .client
        .call_detailed(Call::StorageListLocations, CallOptions::default())
        .await
        .expect("the locations read")
    {
        Reply::StorageListLocations(list) => list,
        other => panic!("not a locations reply: {other:?}"),
    }
}

fn folder(site: &Site, name: &str) -> PathBuf {
    let path = site.root.path().join(name);
    fs::create_dir_all(&path).expect("the folder can be made");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("the mode can be set");
    path
}

fn path_target(path: &Path) -> LocationTarget {
    LocationTarget::Path(path.display().to_string())
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path)
        .expect("the path exists")
        .permissions()
        .mode()
        & 0o777
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_data_folder_takes_the_next_instances_and_moves_nothing() {
    // covers: AC-19, AC-25
    let site = Site::new();
    site.unpin_fake_package();
    let daemon = site.start_with(fast_with_catalog()).await;
    let api = site.api().await;
    let old = api.create_ready("orders", json!({})).await;
    let old_data = api.detail(old).await.expect("the detail").data_path;
    let disk = folder(&site, "disk");

    let chosen = set_location(&api, LocationKind::Data, path_target(&disk), &key())
        .await
        .expect("the folder becomes the default");

    let stackly = fs::canonicalize(&disk)
        .expect("the folder resolves")
        .join("Stackly");
    assert_eq!(chosen.path, stackly.display().to_string());
    assert!(chosen.is_default && !chosen.built_in && chosen.reachable);
    assert_eq!(mode(&stackly), 0o700);
    assert_eq!(mode(&stackly.join("instances")), 0o700);
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(stackly.join(".stackly-location")).expect("a marker"))
            .expect("the marker is JSON");
    assert_eq!(marker["kind"], "data");
    assert_eq!(marker["id"], chosen.id.to_string());
    assert_eq!(mode(&stackly.join(".stackly-location")), 0o600);

    let form = api.form(api.runtime_id, None).await.expect("the form");
    assert_eq!(form.data_location, stackly.display().to_string());

    let new = api.create_ready("invoices", json!({})).await;
    let new_data = api.detail(new).await.expect("the detail").data_path;
    assert!(
        Path::new(&new_data).starts_with(stackly.join("instances")),
        "{new_data}"
    );
    assert_eq!(
        api.detail(old).await.expect("the detail").data_path,
        old_data,
        "an instance that exists stays where it is"
    );
    assert!(Path::new(&old_data).is_dir());

    let list = locations(&api).await;
    let used = |id| {
        list.items
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.instance_count.get())
    };
    assert_eq!(used(chosen.id), Some(1));
    let built_in = list
        .items
        .iter()
        .find(|item| item.built_in && item.kind == LocationKind::Data)
        .expect("the built in data location");
    assert!(!built_in.is_default);
    assert_eq!(built_in.instance_count.get(), 1);

    // Use Default points at the built in location again and moves nothing.
    let back = set_location(&api, LocationKind::Data, LocationTarget::BuiltIn, &key())
        .await
        .expect("the built in location is the default again");
    assert!(back.built_in && back.is_default);
    assert!(Path::new(&new_data).is_dir());
    drop(api);
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn every_rule_that_a_real_folder_can_break_is_refused_with_its_code_and_nothing_changes() {
    // covers: AC-19
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let before = settings_of(&api).await;
    let refused = |rule: &str, error: Box<ErrorBody>| {
        assert_eq!(error.code(), "storage.location_invalid", "{rule}");
        let detail = |key: &str| {
            error
                .details()
                .iter()
                .find(|detail| detail.key() == key)
                .map(|detail| detail.value().to_owned())
        };
        assert_eq!(detail("rule").as_deref(), Some(rule));
        assert!(detail("path").is_some());
    };

    refused(
        "not_absolute",
        set_location(
            &api,
            LocationKind::Data,
            LocationTarget::Path("work/folder".into()),
            &key(),
        )
        .await
        .expect_err("a relative path is refused"),
    );
    refused(
        "missing",
        set_location(
            &api,
            LocationKind::Data,
            path_target(&site.root.path().join("nothing")),
            &key(),
        )
        .await
        .expect_err("a missing folder is refused"),
    );
    let file = site.root.path().join("a-file");
    fs::write(&file, b"x").expect("a file");
    refused(
        "not_directory",
        set_location(&api, LocationKind::Data, path_target(&file), &key())
            .await
            .expect_err("a file is refused"),
    );
    let real = folder(&site, "real");
    let link = site.root.path().join("alias");
    symlink(&real, &link).expect("a symlink");
    refused(
        "symlink",
        set_location(&api, LocationKind::Data, path_target(&link), &key())
            .await
            .expect_err("a link is refused"),
    );
    let open = folder(&site, "open");
    fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).expect("an open mode");
    refused(
        "too_open",
        set_location(&api, LocationKind::Data, path_target(&open), &key())
            .await
            .expect_err("an open folder is refused"),
    );
    let foreign = folder(&site, "foreign");
    fs::create_dir(foreign.join("Stackly")).expect("a Stackly folder of someone else");
    refused(
        "foreign_marker",
        set_location(&api, LocationKind::Data, path_target(&foreign), &key())
            .await
            .expect_err("an unmarked Stackly folder is refused"),
    );
    // A `Stackly` folder inside the data root of the daemon overlaps it.
    let data_root = PathBuf::from(&before.locations.data.path);
    refused(
        "overlaps_root",
        set_location(&api, LocationKind::Logs, path_target(&data_root), &key())
            .await
            .expect_err("a folder over the roots is refused"),
    );

    assert_eq!(settings_of(&api).await, before, "nothing was written");
    drop(api);
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_in_a_protected_folder_of_the_home_the_daemon_was_given_is_refused() {
    // covers: AC-19
    let site = Site::new();
    let home = folder(&site, "home");
    let mut config = fast();
    config.home = Some(home.clone());
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let documents = folder(&site, "home/Documents");
    let inside = folder(&site, "home/Documents/stackly");
    let outside = folder(&site, "home/Work");
    assert!(documents.is_dir());

    let refused = set_location(&api, LocationKind::Data, path_target(&inside), &key())
        .await
        .expect_err("a folder inside Documents is refused");
    assert_eq!(refused.code(), "storage.location_invalid");
    let rule = refused
        .details()
        .iter()
        .find(|detail| detail.key() == "rule")
        .map(|detail| detail.value().to_owned());
    assert_eq!(rule.as_deref(), Some("protected_folder"));
    set_location(&api, LocationKind::Data, path_target(&outside), &key())
        .await
        .expect("a folder elsewhere in the home is taken");
    drop(api);
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_revision_or_the_same_folder_twice_never_adds_a_second_location() {
    // covers: AC-17, AC-19
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let disk = folder(&site, "disk");
    let first = settings_of(&api).await.revision;

    let chosen = set_location_at(&api, LocationKind::Logs, path_target(&disk), first, &key())
        .await
        .expect("the folder becomes the log default");
    let stale = set_location_at(&api, LocationKind::Logs, path_target(&disk), first, &key())
        .await
        .expect_err("the old revision is refused");
    assert_eq!(stale.code(), "resource.revision_mismatch");

    // The same folder for the same kind is used again, not added again.
    set_location(&api, LocationKind::Logs, LocationTarget::BuiltIn, &key())
        .await
        .expect("back to the built in logs");
    let again = set_location(&api, LocationKind::Logs, path_target(&disk), &key())
        .await
        .expect("the same folder is used again");
    assert_eq!(again.id, chosen.id);
    let custom = locations(&api)
        .await
        .items
        .iter()
        .filter(|item| !item.built_in)
        .count();
    assert_eq!(custom, 1);
    drop(api);
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cache_folder_gets_a_downloads_folder_and_is_listed_as_the_default() {
    // covers: AC-19
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let disk = folder(&site, "cache-disk");

    let chosen = set_location(&api, LocationKind::Cache, path_target(&disk), &key())
        .await
        .expect("the cache folder becomes the default");

    let stackly = fs::canonicalize(&disk).expect("resolves").join("Stackly");
    assert!(stackly.join("downloads").is_dir());
    assert!(!stackly.join("instances").exists());
    assert_eq!(chosen.instance_count.get(), 0);
    let view = settings_of(&api).await;
    assert_eq!(view.locations.cache.id, chosen.id);
    drop(api);
    stop_daemon(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_that_went_away_stops_a_start_and_a_create_says_so_before_it_runs() {
    // covers: AC-20
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let disk = folder(&site, "unplugged");
    let chosen = set_location(&api, LocationKind::Data, path_target(&disk), &key())
        .await
        .expect("the folder becomes the default");
    let id = api.create_ready("orders", json!({})).await;
    let stackly = PathBuf::from(&chosen.path);
    fs::remove_dir_all(&stackly).expect("the disk goes away");

    // The preflight names the row, and the create is refused.
    let preflight = api
        .preflight(api.preflight_params("later", json!({})))
        .await
        .expect("the preflight answers");
    let row = preflight
        .checks
        .iter()
        .find(|check| check.kind == PreflightCheckKind::StorageLocation)
        .expect("a storage location row");
    assert_eq!(row.status, PreflightStatus::Error);
    let refused = api
        .create(api.params("later", json!({})))
        .await
        .expect_err("a create into a gone folder is refused");
    assert_eq!(refused.code(), "instance.storage_location_missing");
    assert!(!stackly.exists(), "the create made nothing");

    // A start fails before it spawns a process.
    let started = api.start(id).await;
    let code = match started {
        Err(error) => error.code().to_owned(),
        Ok(accepted) => {
            let operation = accepted
                .accepted
                .expect("a start starts an operation")
                .operation
                .id;
            Api::failure_code(&api.wait_final(operation).await)
        }
    };
    assert_eq!(code, "instance.storage_location_missing");
    drop(api);
    stop_daemon(daemon).await;
}
