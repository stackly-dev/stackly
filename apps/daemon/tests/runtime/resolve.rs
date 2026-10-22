//! `runtime.resolve` and the refusals `runtime.install` gives before any
//! operation exists (spec 0018 AC-1, AC-2, AC-10).

use stackly_core::catalog::testing::CatalogFixture;
use stackly_core::catalog::{Catalog, CatalogEntry, PlannedEntry, TileColor, TileHint};
use stackly_core::provider::Category;
use stackly_protocol::catalog::{SupportReason, SupportStatus, TargetArch, TargetOs};
use stackly_protocol::codes::{catalog as catalog_codes, runtime as runtime_codes};
use stackly_protocol::operation::ListOperationsParams;
use stackly_protocol::runtime::{ArtifactFormat, InstallRuntimeParams};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};

use crate::support::{Site, archive, call_with_key, catalog, client, digest, nats, package};

fn detail(error: &stackly_protocol::ErrorBody, key: &str) -> Option<String> {
    error
        .details()
        .iter()
        .find(|detail| detail.key() == key)
        .map(|detail| detail.value().to_owned())
}

async fn operations_listed(client: &stackly_ipc::Client) -> usize {
    let Reply::OperationList(page) = client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .expect("operations can be listed")
    else {
        panic!("expected an operation.list reply");
    };
    page.items.len()
}

#[tokio::test]
async fn a_selector_resolves_to_the_artifact_by_stable_line_version_and_digest() {
    let ev = evidence!(
        covers = ["AC-001"],
        kind = Kind::Fixture,
        expected = "stable, a line, an exact version, and sha256:<hex> all name the one artifact, with its facts and a status of unverified",
    );
    let site = Site::new();
    let archive = archive();
    let _daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = client(&site).await;
    let sha = digest(&archive).to_string();

    for selector in [
        "stable".to_owned(),
        "2.15".to_owned(),
        "2.15.1".to_owned(),
        format!("sha256:{sha}"),
    ] {
        let Reply::RuntimeResolve(resolved) = client
            .call(Call::RuntimeResolve(nats(&selector)))
            .await
            .unwrap_or_else(|error| panic!("{selector}: {error:?}"))
        else {
            panic!("expected a runtime.resolve reply");
        };
        assert_eq!(resolved.provider, "official/nats", "{selector}");
        assert_eq!(resolved.software, "nats");
        assert_eq!(resolved.artifact_id, "nats-2.15.1-macos-aarch64");
        assert_eq!(
            (resolved.version.as_str(), resolved.line.as_str()),
            ("2.15.1", "2.15")
        );
        assert_eq!(resolved.sha256, sha);
        assert_eq!(resolved.size.get(), archive.len() as u64);
        assert_eq!(
            resolved.unpacked_size.map(|size| size.get()),
            Some(1_048_576)
        );
        assert_eq!(resolved.format, ArtifactFormat::TarGz);
        assert_eq!(resolved.min_os, "13.0");
        assert!(resolved.stable);
        assert_eq!(resolved.target.os, TargetOs::Macos);
        assert_eq!(resolved.target.arch, TargetArch::Aarch64);
        assert_eq!(resolved.support, SupportStatus::Unverified);
        assert_eq!(resolved.support_reason, SupportReason::Untested);
    }
    ev.pass();
}

#[tokio::test]
async fn a_selector_that_matches_nothing_or_names_another_artifact_is_refused() {
    let ev = evidence!(
        covers = ["AC-001"],
        kind = Kind::Fixture,
        expected = "an unknown line, digest, or provider and a changed digest are refused with their codes, details, and static hints",
    );
    let site = Site::new();
    let archive = archive();
    let _daemon = site.start(catalog(vec![package(&archive)])).await;
    let client = client(&site).await;

    let unresolved = client
        .call(Call::RuntimeResolve(nats("19")))
        .await
        .unwrap_err();
    assert_eq!(unresolved.code(), runtime_codes::SELECTOR_UNRESOLVED.code);
    assert_eq!(detail(&unresolved, "reason").as_deref(), Some("no_match"));
    assert_eq!(detail(&unresolved, "knownLines").as_deref(), Some("2.15"));
    assert_eq!(
        unresolved.hint(),
        Some("Run stackly provider inspect for this provider to see its versions.")
    );

    let no_digest = client
        .call(Call::RuntimeResolve(nats(&format!(
            "sha256:{}",
            "0".repeat(64)
        ))))
        .await
        .unwrap_err();
    assert_eq!(no_digest.code(), runtime_codes::SELECTOR_UNRESOLVED.code);

    let changed = client
        .call(Call::RuntimeResolve(InstallRuntimeParams {
            expected_sha256: Some("1".repeat(64)),
            ..nats("stable")
        }))
        .await
        .unwrap_err();
    assert_eq!(changed.code(), runtime_codes::SELECTOR_CHANGED.code);
    assert_eq!(detail(&changed, "expectedSha256"), Some("1".repeat(64)));
    assert_eq!(
        detail(&changed, "resolvedSha256"),
        Some(digest(&archive).to_string())
    );

    let unknown = client
        .call(Call::RuntimeResolve(InstallRuntimeParams {
            provider: "nats".to_owned(),
            ..nats("stable")
        }))
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), catalog_codes::PROVIDER_NOT_FOUND.code);
    assert_eq!(
        detail(&unknown, "suggestion").as_deref(),
        Some("official/nats")
    );

    let malformed = client
        .call(Call::RuntimeResolve(nats("1..2")))
        .await
        .unwrap_err();
    assert_eq!(malformed.code(), "request.invalid_params");
    ev.pass();
}

// covers: AC-10
#[tokio::test]
async fn runtime_install_refuses_an_artifact_for_another_target() {
    let ev = evidence!(
        covers = ["AC-002", "AC-005"],
        kind = Kind::Fixture,
        expected = "a provider whose only build is for linux or for intel is refused with catalog.not_installable and the reason, and no operation is created",
    );
    let site = Site::new();
    let _daemon = site
        .start(catalog(vec![
            CatalogFixture::LinuxOnly.package(),
            CatalogFixture::IntelOnly.package(),
        ]))
        .await;
    let client = client(&site).await;

    for (provider, reason, targets) in [
        ("official/linuxonly", "no_build_for_host", "linux-x86_64"),
        ("official/intelonly", "no_build_for_arch", "macos-x86_64"),
    ] {
        let error = call_with_key(
            &client,
            Call::RuntimeInstall(InstallRuntimeParams {
                provider: provider.to_owned(),
                ..nats("2.15.1")
            }),
            &format!("install-{reason}-0001"),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.code(),
            catalog_codes::NOT_INSTALLABLE.code,
            "{provider}"
        );
        assert_eq!(
            detail(&error, "reason").as_deref(),
            Some(reason),
            "{provider}"
        );
        assert_eq!(detail(&error, "availableTargets").as_deref(), Some(targets));
        assert!(error.hint().is_some());
    }
    assert_eq!(operations_listed(&client).await, 0);
    ev.pass();
}

#[tokio::test]
async fn a_planned_provider_is_refused_with_the_selector_as_its_version() {
    let ev = evidence!(
        covers = ["AC-010"],
        kind = Kind::Fixture,
        expected = "a planned entry is refused with provider_planned, the raw selector as the version, and no available target",
    );
    let site = Site::new();
    let planned = CatalogEntry::Planned(PlannedEntry {
        id: "official/valkey".parse().unwrap(),
        display_name: "Valkey".to_owned(),
        category: Category::Cache,
        description: "In-memory store.".to_owned(),
        note: None,
        tile: TileHint {
            mark: "Vk".to_owned(),
            tile: TileColor::Red,
        },
    });
    let _daemon = site.start(Catalog::for_tests(&[planned])).await;
    let client = client(&site).await;

    let error = client
        .call(Call::RuntimeResolve(InstallRuntimeParams {
            provider: "official/valkey".to_owned(),
            ..nats("8.1")
        }))
        .await
        .unwrap_err();

    assert_eq!(error.code(), catalog_codes::NOT_INSTALLABLE.code);
    assert_eq!(
        detail(&error, "reason").as_deref(),
        Some("provider_planned")
    );
    assert_eq!(detail(&error, "version").as_deref(), Some("8.1"));
    assert_eq!(detail(&error, "availableTargets").as_deref(), Some(""));
    ev.pass();
}
