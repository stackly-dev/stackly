//! `instance.get_create_form` with a revision through the real daemon (spec
//! 0020 AC-1, update 2026-11-01): the form reads the package create would
//! use, the pinned one when the catalog has another revision or none, and
//! refuses with the code create reports for the same name, revision, and
//! runtime.

use std::path::{Path, PathBuf};

use serde_json::json;
use stackly_core::catalog::{Catalog, CatalogEntry, PlannedEntry, TileColor, TileHint};
use stackly_core::provider::Category;
use stackly_protocol::instance::{CreateProviderSource, FieldKind};
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::server::DaemonConfig;

use crate::support::{Api, Site, edited_fake_package, fake_catalog, fast};

/// The code of the first error row of a preflight of the fake draft with
/// `provider` and `revision`.
async fn preflight_code(api: &Api, provider: &str, revision: Option<&str>) -> Option<String> {
    let mut params = api.preflight_params("pinned-form", json!({}));
    params.provider.name = provider.to_owned();
    params.provider.revision = revision.map(str::to_owned);
    let preflight = api
        .preflight(params)
        .await
        .expect("a preflight is answered");
    preflight
        .checks
        .iter()
        .find_map(|check| check.error.as_ref())
        .map(|error| error.code().to_owned())
}

/// The form code for `provider` at `revision`, and the same code from a
/// preflight of that draft.
async fn assert_form_refuses_like_create(
    api: &Api,
    runtime_id: uuid::Uuid,
    provider: &str,
    revision: &str,
    code: &str,
) {
    let error = api
        .form_at(runtime_id, Some(provider), Some(revision))
        .await
        .expect_err("the form is refused");
    assert_eq!(error.code(), code, "{error:?}");
    assert_eq!(
        preflight_code(api, provider, Some(revision))
            .await
            .as_deref(),
        Some(code),
        "create reports the same code for {provider} {revision}"
    );
}

/// Every file under `dir`, relative to it, sorted.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            out.push(path.strip_prefix(base).expect("under the base").to_owned());
            if path.is_dir() {
                walk(base, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn with_catalog(catalog: Catalog) -> DaemonConfig {
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(catalog));
    config
}

async fn stop(api: Api, daemon: stacklyd::server::DaemonHandle) {
    drop(api);
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn without_a_catalog_the_form_of_a_pinned_revision_reads_the_pinned_package() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "with no catalog, get_create_form for testing/fake 0.3.0 answers the pinned package with ignoreTerm as a boolean and readyDelayMs as an integer, refuses without a provider with catalog.unavailable and an unpinned revision with instance.provider_not_found like create, and writes no file",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let providers = site.prepared.layout().providers_dir();
    let before = files_under(&providers);

    let form = api
        .form_at(site.runtime_id, Some("testing/fake"), Some("0.3.0"))
        .await
        .expect("the form of the pinned package is built");

    assert_eq!(form.provider.source, CreateProviderSource::Pinned);
    assert_eq!(form.provider.revision, "0.3.0");
    let kind = |key: &str| {
        form.fields
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.kind)
    };
    assert_eq!(kind("ignoreTerm"), Some(FieldKind::Boolean));
    assert_eq!(kind("readyDelayMs"), Some(FieldKind::Integer));
    assert!(
        form.suggested_name
            .as_deref()
            .is_some_and(|name| name.starts_with("fake-")),
        "{:?}",
        form.suggested_name
    );

    let error = api
        .form_at(site.runtime_id, None, Some("0.3.0"))
        .await
        .expect_err("no provider and no catalog");
    assert_eq!(error.code(), "catalog.unavailable", "{error:?}");
    assert_eq!(error.detail("reason"), Some("not_configured"));

    assert_form_refuses_like_create(
        &api,
        site.runtime_id,
        "testing/fake",
        "9.9.9",
        "instance.provider_not_found",
    )
    .await;

    assert_eq!(files_under(&providers), before, "the form pins nothing");
    assert!(api.list(Default::default()).await.items.is_empty());
    stop(api, daemon).await;
    ev.pass();
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn with_a_catalog_the_revision_picks_the_entry_or_the_pinned_package() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "with a loaded catalog, the catalog revision builds the form from the catalog entry, and another pinned revision without a provider picks the name per runtime software and builds from the pinned package",
    );
    let (_scratch, newer) = edited_fake_package(|dir| {
        let manifest = dir.join("provider.yaml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
        std::fs::write(
            &manifest,
            text.replace("revision: \"0.3.0\"", "revision: \"0.3.1\""),
        )
        .expect("the manifest is written");
    });
    let site = Site::new();
    site.pin_package(&newer);
    let daemon = site.start_with(with_catalog(fake_catalog())).await;
    let api = site.api().await;

    let catalog = api
        .form_at(site.runtime_id, Some("testing/fake"), Some("0.3.0"))
        .await
        .expect("the form of the catalog revision is built");
    assert_eq!(
        catalog.provider.source,
        CreateProviderSource::DevelopmentCatalog
    );
    assert_eq!(catalog.provider.revision, "0.3.0");

    let pinned = api
        .form_at(site.runtime_id, None, Some("0.3.1"))
        .await
        .expect("the form of the pinned revision is built");
    assert_eq!(pinned.provider.name, "testing/fake");
    assert_eq!(pinned.provider.source, CreateProviderSource::Pinned);
    assert_eq!(pinned.provider.revision, "0.3.1");
    assert_eq!(
        preflight_code(&api, "testing/fake", Some("0.3.1")).await,
        None
    );
    stop(api, daemon).await;
    ev.pass();
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn a_planned_entry_with_a_pinned_revision_reads_the_pinned_package_like_create() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "a planned catalog entry with a pinned revision builds the form from the pinned package, and create accepts the same draft",
    );
    let site = Site::new();
    let planned = Catalog::for_tests(&[CatalogEntry::Planned(PlannedEntry {
        id: "testing/fake".parse().expect("a provider id"),
        display_name: "Fake Provider".to_owned(),
        category: Category::Testing,
        description: "A planned fake.".to_owned(),
        note: None,
        tile: TileHint {
            mark: "Fk".to_owned(),
            tile: TileColor::Blue,
        },
    })]);
    let daemon = site.start_with(with_catalog(planned)).await;
    let api = site.api().await;

    let form = api
        .form_at(site.runtime_id, Some("testing/fake"), Some("0.3.0"))
        .await
        .expect("the form of the pinned package is built");
    assert_eq!(form.provider.source, CreateProviderSource::Pinned);
    assert_eq!(
        preflight_code(&api, "testing/fake", Some("0.3.0")).await,
        None
    );

    let error = api
        .form(site.runtime_id, Some("testing/fake"))
        .await
        .expect_err("a planned entry without a revision is unavailable");
    assert_eq!(error.code(), "instance.provider_unavailable", "{error:?}");
    stop(api, daemon).await;
    ev.pass();
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn two_digest_folders_and_other_software_refuse_the_form_like_create() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "two digest folders under one pinned revision answer instance.provider_ambiguous, and a pinned package of other software answers instance.runtime_mismatch, each the code create reports for the same draft",
    );
    let (_scratch, other) = edited_fake_package(|dir| {
        let manifest = dir.join("provider.yaml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
        std::fs::write(
            &manifest,
            text.replace("id: testing/fake", "id: testing/other")
                .replace("software: fake", "software: other"),
        )
        .expect("the manifest is written");
        let versions = dir.join("versions.json");
        let text = std::fs::read_to_string(&versions).expect("the versions read");
        std::fs::write(
            &versions,
            text.replace("\"software\": \"fake\"", "\"software\": \"other\""),
        )
        .expect("the versions are written");
    });
    let site = Site::new();
    site.pin_package(&other);
    let revision = site
        .prepared
        .layout()
        .providers_dir()
        .join("testing/fake/0.3.0");
    let digest = std::fs::read_dir(&revision)
        .expect("the pinned revision folder reads")
        .flatten()
        .next()
        .expect("the seed pinned one digest folder")
        .path();
    let copy = revision.join("0".repeat(64));
    std::fs::create_dir(&copy).expect("a second digest folder is made");
    for entry in std::fs::read_dir(&digest)
        .expect("the digest folder reads")
        .flatten()
    {
        if entry.path().is_file() {
            std::fs::copy(entry.path(), copy.join(entry.file_name())).expect("a file copies");
        }
    }
    let daemon = site.start().await;
    let api = site.api().await;

    assert_form_refuses_like_create(
        &api,
        site.runtime_id,
        "testing/fake",
        "0.3.0",
        "instance.provider_ambiguous",
    )
    .await;

    let error = api
        .form_at(site.runtime_id, Some("testing/other"), Some("0.3.0"))
        .await
        .expect_err("the pinned package runs other software");
    assert_eq!(error.code(), "instance.runtime_mismatch", "{error:?}");
    assert_eq!(error.detail("providerSoftware"), Some("other"), "{error:?}");
    assert_eq!(
        preflight_code(&api, "testing/other", Some("0.3.0"))
            .await
            .as_deref(),
        Some("instance.runtime_mismatch")
    );
    stop(api, daemon).await;
    ev.pass();
}
