//! The runtime install on the real artifacts (spec 0018, milestone 5,
//! AC-20, AC-6, AC-12): the in process daemon installs each `available`
//! line whose provider package exists from the runtime cache of `cargo
//! xtask`, with no network, onto a `TestRoot`, through the real macOS
//! adapters. Every test is native: `#[ignore = "native: ..."]`, run with
//! `cargo test -p stacklyd --test native_runtime_install -- --ignored`
//! (`cargo xtask evidence run --kind native -- ...`). An artifact that is not
//! in the cache ends a test as `BLOCKED`, never as a download; the one test
//! that downloads, `nats_installs_from_the_network`, stays `BLOCKED` until
//! `STACKLY_NATIVE_NETWORK=1` says the network run was approved.
#![cfg(target_os = "macos")]

#[path = "native_runtime_install/harness.rs"]
mod harness;
#[path = "native_runtime_install/recorders.rs"]
mod recorders;
#[path = "native_runtime_install/signature.rs"]
mod signature;

use std::path::{Path, PathBuf};
use std::process::Command;

use stackly_protocol::runtime::{GetRuntimeParams, InstallOutcome, RuntimeState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};

use crate::harness::{
    Line, NATS, OWNER, POSTGRESQL_17, POSTGRESQL_18, Site, TRAEFIK, artifact_facts, cached,
    install, install_params, wait_end,
};
use crate::recorders::Asked;
use stackly_platform::ChildRole;
use stackly_protocol::operation::OperationState;

/// The reason MySQL, ClickHouse, and Valkey wait for their providers.
const NO_PACKAGE: &str = "no provider package (features 40 to 43)";
/// The variable that says the network run was approved.
const NETWORK_APPROVED: &str = "STACKLY_NATIVE_NETWORK";

/// The lines of the file that carry `com.apple.quarantine` under `dir`.
fn quarantined(dir: &Path) -> Vec<String> {
    let output = Command::new("/usr/bin/xattr")
        .arg("-lr")
        .arg(dir)
        .output()
        .expect("xattr runs");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("com.apple.quarantine"))
        .map(str::to_owned)
        .collect()
}

/// Installs `line` from the download cache the daemon was given, or from the
/// network when `from_network`, and checks what AC-20 asks of the result.
async fn install_line(ev: &Evidence, line: Line, from_network: bool) {
    let Some((sha256, size)) = cached(line).or_else(|| from_network.then(|| artifact_facts(line)))
    else {
        ev.blocked(
            &format!("artifact {} is not in the runtime cache", line.artifact),
            OWNER,
        );
        return;
    };
    let site = if from_network {
        Site::online()
    } else {
        Site::offline()
    };
    if !from_network {
        site.fill_download_cache(line);
    }
    let _daemon = site.start(line.software).await;
    let client = site.client().await;

    let reply = install(
        &client,
        install_params(line.software, line.selector),
        &format!("native-{}", line.artifact),
    )
    .await;
    assert_eq!(reply.resolved.artifact_id, line.artifact);
    assert_eq!(reply.resolved.sha256, sha256);
    assert_eq!(reply.resolved.size.get(), size);
    let InstallOutcome::Started(accepted) = reply.outcome else {
        panic!("a new root has nothing installed, so the install starts");
    };
    let runtime_id = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the runtime")
        .id
        .clone();

    let done = wait_end(&client, accepted.operation.id).await;
    assert_eq!(
        done.operation.state,
        OperationState::Succeeded,
        "the install ended as {:?}",
        done.operation
    );

    let Reply::RuntimeGet(detail) = client
        .call(Call::RuntimeGet(GetRuntimeParams {
            runtime_id: runtime_id.parse().expect("a runtime id"),
        }))
        .await
        .expect("the runtime can be read")
    else {
        panic!("expected a runtime.get reply");
    };
    assert_eq!(detail.summary.state, RuntimeState::Installed);
    assert_eq!(detail.summary.software, line.software);
    assert_eq!(detail.summary.version, line.version);
    assert_eq!(detail.summary.artifact_sha256.as_deref(), Some(&*sha256));
    let runtime_dir = PathBuf::from(&detail.path);
    assert!(runtime_dir.is_dir(), "{} exists", runtime_dir.display());
    assert!(runtime_dir.starts_with(site.root.path()));

    // No installed file carries com.apple.quarantine (AC-20).
    assert_eq!(quarantined(&runtime_dir), Vec::<String>::new());

    let spawned = site.spawned.lock().expect("no test thread panics").clone();
    assert!(!spawned.is_empty(), "the exec check ran the binary");
    for spawn in &spawned {
        // The daemon starts the executables of the artifact and nothing
        // else, each as the exec check, with the fixed four variable environment.
        assert_eq!(spawn.role, ChildRole::RuntimeCheck, "{spawn:?}");
        // Nothing is inherited from the daemon: four variables, the two
        // folders in the scratch of this root.
        let names: Vec<&str> = spawn.env.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["PATH", "HOME", "TMPDIR", "LANG"], "{spawn:?}");
        for (name, value) in &spawn.env {
            if name == "HOME" || name == "TMPDIR" {
                assert!(Path::new(value).starts_with(site.root.path()), "{spawn:?}");
            }
        }
        assert!(spawn.program.starts_with(site.root.path()), "{spawn:?}");
        let name = spawn.program.file_name().expect("a program has a name");
        assert!(
            detail
                .executables
                .iter()
                .any(|executable| Path::new(executable).file_name() == Some(name)),
            "{spawn:?} is not one of {:?}",
            detail.executables
        );
    }
    // The only tool the signing port reaches is /usr/bin/codesign: the
    // adapter has no other program, and every path it was asked about is a
    // file of this root.
    let asked = site.asked.lock().expect("no test thread panics").clone();
    assert!(!asked.is_empty(), "the install checked the signatures");
    for question in &asked {
        let paths: Vec<&PathBuf> = match question {
            Asked::Assess(path) | Asked::Quarantine(path) => vec![path],
            Asked::Sign(paths) => paths.iter().collect(),
        };
        assert!(paths.iter().all(|path| path.starts_with(site.root.path())));
    }
    let urls = site.fetched.lock().expect("no test thread panics").clone();
    if !from_network {
        assert_eq!(urls, Vec::<String>::new(), "nothing was fetched");
    }

    // The installed executable, run once more as the exec check ran it,
    // reports the version of the artifact.
    let executable = runtime_dir.join(&detail.executables[0]);
    let output = Command::new(&executable)
        .args(&spawned[0].args)
        .env_clear()
        .output()
        .expect("the installed executable runs");
    let printed = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    assert!(
        printed.contains(line.version),
        "{} printed {printed:?}",
        executable.display()
    );

    ev.subject(
        line.software,
        &detail.summary.version,
        Some(&sha256),
        Source::Managed,
    );
    ev.pass_with(&format!(
        "{} {} installed {} to a TestRoot ({} exec check(s), {} codesign question(s), {} fetch(es)), no file with com.apple.quarantine",
        line.software,
        detail.summary.version,
        if from_network {
            "from the network"
        } else {
            "from the cache with no network"
        },
        spawned.len(),
        asked.len(),
        urls.len()
    ));
}

macro_rules! cache_line_test {
    ($name:ident, $line:expr, $expected:literal) => {
        #[tokio::test]
        #[ignore = "native: needs the artifact in the runtime cache of cargo xtask"]
        async fn $name() {
            let ev = evidence!(
                covers = ["REQ-004", "REQ-005", "AC-049"],
                kind = Kind::Native,
                expected = $expected,
            );
            install_line(&ev, $line, false).await;
        }
    };
}

cache_line_test!(
    nats_2_15_installs_from_the_cache,
    NATS,
    "the daemon installs NATS 2.15.1 from the runtime cache with no network to an installed runtime whose exec check printed the artifact version, with no com.apple.quarantine on any file and only codesign and the artifact executable run"
);
cache_line_test!(
    traefik_3_7_installs_from_the_cache,
    TRAEFIK,
    "the daemon installs Traefik 3.7.15 from the runtime cache with no network to an installed runtime whose exec check printed the artifact version, with no com.apple.quarantine on any file and only codesign and the artifact executable run"
);
cache_line_test!(
    postgresql_17_installs_from_the_cache,
    POSTGRESQL_17,
    "the daemon installs PostgreSQL 17.11 from the runtime cache with no network to an installed runtime whose exec check printed the artifact version, with no com.apple.quarantine on any file and only codesign and the artifact executable run"
);
cache_line_test!(
    postgresql_18_installs_from_the_cache,
    POSTGRESQL_18,
    "the daemon installs PostgreSQL 18.6 from the runtime cache with no network to an installed runtime whose exec check printed the artifact version, with no com.apple.quarantine on any file and only codesign and the artifact executable run"
);

macro_rules! blocked_line_test {
    ($name:ident, $what:literal) => {
        #[test]
        #[ignore = "native: waits for the provider package of features 40 to 43"]
        fn $name() {
            let ev = evidence!(
                covers = ["REQ-004", "REQ-005", "AC-049"],
                kind = Kind::Native,
                expected = concat!(
                    "the daemon installs ",
                    $what,
                    " from the runtime cache with no network to an installed runtime"
                ),
            );
            ev.blocked(NO_PACKAGE, OWNER);
        }
    };
}

blocked_line_test!(mysql_8_4_installs_from_the_cache, "MySQL 8.4.11");
blocked_line_test!(mysql_9_7_installs_from_the_cache, "MySQL 9.7.2");
blocked_line_test!(clickhouse_installs_from_the_cache, "ClickHouse 26.8.30.8");
blocked_line_test!(valkey_9_1_installs_from_the_cache, "Valkey 9.1.2");

#[tokio::test]
#[ignore = "native: downloads NATS 2.15.1 (6.8 MB) from GitHub Releases; needs STACKLY_NATIVE_NETWORK=1"]
async fn nats_installs_from_the_network() {
    let ev = evidence!(
        covers = ["REQ-004", "REQ-005", "AC-049"],
        kind = Kind::Native,
        expected = "the daemon downloads NATS 2.15.1 over https into an empty TestRoot and installs it to an installed runtime whose exec check printed the artifact version, with no com.apple.quarantine on any file",
    );
    if std::env::var_os(NETWORK_APPROVED).is_none_or(|value| value != "1") {
        ev.blocked("network run not approved", OWNER);
        return;
    }
    install_line(&ev, NATS, true).await;
}

#[tokio::test]
#[ignore = "native: runs the real adapters on a TestRoot with no network and an empty download cache"]
async fn an_install_without_a_network_and_without_a_cache_entry_is_offline_not_a_hang() {
    let ev = evidence!(
        covers = ["AC-046"],
        kind = Kind::Native,
        expected = "an empty download cache and a network that cannot be reached end the install in the Download stage with runtime.download_offline, within the retry time, and leave no runtime",
    );
    let site = Site::offline();
    let _daemon = site.start(NATS.software).await;
    let client = site.client().await;

    let reply = install(
        &client,
        install_params(NATS.software, NATS.selector),
        "native-offline",
    )
    .await;
    let InstallOutcome::Started(accepted) = reply.outcome else {
        panic!("a new root has nothing installed, so the install starts");
    };
    let started = std::time::Instant::now();
    let done = wait_end(&client, accepted.operation.id).await;

    assert_eq!(done.operation.state, OperationState::Failed);
    let failure = done.operation.failure.as_ref().expect("a failure");
    assert_eq!(failure.error.code(), "runtime.download_offline");
    assert!(failure.retryable, "{failure:?}");
    assert!(
        !site
            .fetched
            .lock()
            .expect("no test thread panics")
            .is_empty()
    );
    // The artifact the install tried to fetch is the subject; it never ran.
    let (sha256, _) = artifact_facts(NATS);
    ev.subject(NATS.software, NATS.version, Some(&sha256), Source::Managed);
    ev.pass_with(&format!(
        "runtime.download_offline after {:?}, retryable, no runtime",
        started.elapsed()
    ));
}
