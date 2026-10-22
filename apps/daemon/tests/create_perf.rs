//! `instance.preflight` and `instance.create` on this Mac with ten instances
//! registered (spec 0020 AC-24, NFR-006): the reply of a create comes within
//! 500 ms and the one of a preflight within 300 ms at the 95th percentile,
//! for a package with two IP endpoints whose ports are fixed (so no
//! suggestion search runs), also while a Keychain call from another action
//! waits behind its dialog.
#![cfg(target_os = "macos")]

#[path = "common/mod.rs"]
mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_core::catalog::{Catalog, CatalogEntry, ProviderEntry, TileColor, TileHint};
use stackly_core::provider::{NameCheck, ValidatedPackage, read_package};
use stackly_protocol::Call;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, PortRequest, PreflightCreateParams, SecretPlan,
    SecretSource,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::{SecretInput, SecretMode, SecretText, SetSecretParams};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{TestRoot, allocate_port};
use stacklyd::catalog::CatalogOverride;

use crate::support::{Api, Site, fast};

const REGISTERED: usize = 10;
const PREFLIGHTS: usize = 40;
const CREATES: usize = 10;
const HELD_CREATES: usize = 5;

/// The bound a timing is checked against: the spec's own in an evidence run
/// (`cargo xtask evidence run`, which records the numbers), and ten times
/// looser in a plain `cargo test`, where other work on this Mac stretches
/// every fsync. The loose bound still catches a reply that waits on the
/// Keychain or walks the registry once per instance.
fn bound(spec: Duration) -> Duration {
    if std::env::var_os(stackly_testkit::evidence::EVIDENCE_DIR_ENV).is_some() {
        spec
    } else {
        spec * 10
    }
}

fn p95(times: &mut [Duration]) -> Duration {
    times.sort();
    times[(times.len() * 95).div_ceil(100) - 1]
}

/// Copies the folder `from` to `to`.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy folder can be created");
    for entry in std::fs::read_dir(from).expect("the folder can be read") {
        let entry = entry.expect("an entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("the file can be copied");
        }
    }
}

/// A catalog that holds the fake package with a second IP endpoint, `admin`,
/// under a newer revision, so the pin does not collide with the seeded one.
/// `scratch` holds the edited copy.
fn two_endpoint_catalog(scratch: &Path) -> Catalog {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let dir = scratch.join("fake");
    copy_tree(&source, &dir);
    let manifest = std::fs::read_to_string(dir.join("provider.yaml")).expect("the manifest reads");
    let edited = manifest
        .replace("revision: \"0.3.0\"", "revision: \"0.3.1\"")
        .replace(
            "    http: { transport: tcp, purpose: client, defaultHost: \"127.0.0.1\", defaultPort: 18080 }\n",
            "    http: { transport: tcp, purpose: client, defaultHost: \"127.0.0.1\", defaultPort: 18080 }\n    admin: { transport: tcp, purpose: admin, defaultHost: \"127.0.0.1\", defaultPort: 18082 }\n",
        );
    assert_ne!(edited, manifest, "the manifest was edited");
    std::fs::write(dir.join("provider.yaml"), edited).expect("the manifest writes");
    let files = read_package(&stackly_platform_macos::MacosFilesystem, &dir)
        .expect("the edited package reads");
    let package = ValidatedPackage::from_files(files, NameCheck::FolderName)
        .expect("the edited package validates");
    Catalog::for_tests(&[CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package),
        tile: TileHint {
            mark: "Fk".to_owned(),
            tile: TileColor::Blue,
        },
        notes: Vec::new(),
    })])
}

/// Two fixed ports that are free.
fn two_ports() -> Vec<EndpointRequest> {
    ["http", "admin"]
        .into_iter()
        .map(|name| EndpointRequest {
            name: name.to_owned(),
            port: PortRequest::Fixed(allocate_port().expect("a free port")),
        })
        .collect()
}

/// The create of `name` on the catalog revision of the package.
fn create_of(api: &Api, name: &str) -> CreateInstanceParams {
    let mut params = api.params(name, json!({}));
    params.provider.revision = None;
    params.endpoints = two_ports();
    params
}

/// The preflight of the same draft.
fn preflight_of(api: &Api, name: &str) -> PreflightCreateParams {
    let create = create_of(api, name);
    PreflightCreateParams {
        name: create.name,
        display_name: create.display_name,
        provider: create.provider,
        runtime_id: create.runtime_id,
        endpoints: create.endpoints,
        settings: create.settings,
        secrets: vec![SecretPlan {
            field: "authToken".to_owned(),
            source: SecretSource::Generate,
        }],
        start: false,
    }
}

async fn timed_preflights(api: &Api, label: &str) -> Duration {
    let mut times = Vec::new();
    for number in 0..PREFLIGHTS {
        let draft = preflight_of(api, &format!("{label}-pre-{number}"));
        let started = Instant::now();
        let preflight = api.preflight(draft).await.expect("a preflight is answered");
        times.push(started.elapsed());
        assert!(preflight.ready, "{:#?}", preflight.checks);
        assert!(
            preflight
                .checks
                .iter()
                .all(|check| check.suggested_port.is_none()),
            "no suggestion search ran"
        );
    }
    p95(&mut times)
}

/// Sends `count` creates and returns the p95 of their replies. With
/// `wait` each operation ends before the next create, so the creates do
/// not race for the generation of the registry; without it the operations
/// end after [`finish`] is called by the caller.
async fn timed_creates(
    api: &Api,
    label: &str,
    count: usize,
    wait: bool,
) -> (Duration, Vec<uuid::Uuid>) {
    let mut times = Vec::new();
    let mut operations = Vec::new();
    for number in 0..count {
        let params = create_of(api, &format!("{label}-{number}"));
        let started = Instant::now();
        let accepted = api
            .create(params)
            .await
            .expect("the create is accepted")
            .accepted
            .expect("a create starts an operation");
        times.push(started.elapsed());
        if wait {
            api.wait_state(accepted.operation.id, OperationState::Succeeded)
                .await;
        } else {
            operations.push(accepted.operation.id);
        }
    }
    (p95(&mut times), operations)
}

/// A Keychain call of another action that waits behind its dialog.
struct Dialog {
    hold: stackly_testkit::SecretHold,
    waiting:
        tokio::task::JoinHandle<Result<stackly_protocol::Reply, Box<stackly_protocol::ErrorBody>>>,
}

impl Dialog {
    /// Opens the dialog: the call of another connection reaches the store
    /// and waits there.
    async fn open(site: &Site, reference: &str) -> Self {
        let hold = site.secrets.hold_next();
        let other = site.api().await;
        let reference = reference.to_owned();
        let waiting = tokio::spawn(async move {
            other
                .client
                .call(Call::SecretSet(SetSecretParams {
                    reference,
                    input: SecretInput::Value(SecretText::new("perf-fixture-value".to_owned())),
                    mode: SecretMode::Create,
                    expected_revision: None,
                    confirm_stored_only: false,
                }))
                .await
        });
        assert!(
            hold.wait_until_held(Duration::from_secs(10)),
            "the other action reached the Keychain and waits"
        );
        Self { hold, waiting }
    }

    /// Answers the dialog and waits for the other action to end.
    async fn close(self) {
        self.hold.release();
        self.waiting
            .await
            .expect("the waiting call ends")
            .expect("the other action succeeds");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn create_and_preflight_with_ten_instances_answer_within_the_bounds_also_behind_a_keychain_dialog()
 {
    let ev = evidence!(
        covers = ["NFR-006"],
        kind = Kind::Fixture,
        expected = "with ten instances registered and a package with two IP endpoints on fixed ports, the p95 of 40 instance.preflight replies is at most 300 ms and the p95 of 10 instance.create replies, plus 5 behind a dialog, at most 500 ms, with no suggestion search, and again while a Keychain call of another action waits behind its dialog",
    );
    let site = Site::new();
    // The seeded pin has the permissions of a test; the create pins again.
    site.unpin_fake_package();
    let scratch = TestRoot::new().expect("a scratch folder can be created");
    let mut config = fast();
    config.catalog = Some(CatalogOverride::Preloaded(two_endpoint_catalog(
        scratch.path(),
    )));
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    for number in 0..REGISTERED {
        let accepted = api
            .create(create_of(&api, &format!("seed-{number}")))
            .await
            .expect("the create is accepted")
            .accepted
            .expect("a create starts an operation");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    }
    let load = std::process::Command::new("/usr/bin/uptime")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();

    // The preflights run with ten instances registered.
    let preflight = timed_preflights(&api, "plain").await;
    let dialog = Dialog::open(&site, "local:perf/first/value").await;
    let preflight_held = timed_preflights(&api, "held").await;
    dialog.close().await;

    // The creates run from ten registered instances up.
    let (create, _) = timed_creates(&api, "plain", CREATES, true).await;
    let dialog = Dialog::open(&site, "local:perf/second/value").await;
    let (create_held, held_operations) = timed_creates(&api, "held", HELD_CREATES, false).await;
    dialog.close().await;
    // The creates were answered while the Keychain waited; they all run
    // when it answers. Five of them register at once, and the registry
    // may refuse one after its five attempts (AC-10): that is the only
    // failure allowed here.
    let mut succeeded = 0;
    for operation in held_operations {
        let ended = api.wait_final(operation).await;
        match ended.operation.state {
            OperationState::Succeeded => succeeded += 1,
            _ => assert_eq!(
                Api::failure_code(&ended),
                "registry.generation_conflict",
                "{ended:?}"
            ),
        }
    }
    assert!(succeeded >= 1, "at least one create ends");

    let summary = format!(
        "p95 of {PREFLIGHTS} preflights with {REGISTERED} registered and of {CREATES} creates (from {REGISTERED} registered) and {HELD_CREATES} creates behind a dialog ({succeeded} of them ended succeeded): preflight {} ms (behind a dialog {} ms), create {} ms (behind a dialog {} ms); {load}",
        preflight.as_millis(),
        preflight_held.as_millis(),
        create.as_millis(),
        create_held.as_millis(),
    );
    eprintln!("{summary}");
    for (label, took, spec) in [
        ("preflight", preflight, 300),
        ("preflight behind a dialog", preflight_held, 300),
        ("create", create, 500),
        ("create behind a dialog", create_held, 500),
    ] {
        assert!(
            took <= bound(Duration::from_millis(spec)),
            "{label}: {summary}"
        );
    }
    drop((api, scratch));
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass_with(&summary);
}
