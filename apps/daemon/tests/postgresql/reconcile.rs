//! A daemon that ends without a plan, with real PostgreSQL clusters (spec
//! 0030, AC-1, AC-2, AC-4): every client closes, the daemon is killed, or
//! the data of an instance is gone. The clusters of the managed runtime
//! from the cache of `cargo xtask`; an artifact that is not there ends a
//! test as `BLOCKED`, never as a download.

use std::collections::BTreeSet;
use std::path::Path;

use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState, InstanceSummary};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::common;
use crate::harness::{Made, PG17, PG18, Pg, Site, create_instance, output_of, tree_digest};
use crate::sentinel::SentinelCluster;
use crate::support::Api;

/// One running cluster of a test and what the daemon said about it.
struct Cluster {
    made: Made,
    sentinel: SentinelCluster,
    value: String,
}

/// Creates the instance `name`, starts it, and writes its own sentinel.
async fn running_cluster(site: &Site, api: &Api, name: &str) -> (Cluster, InstanceSummary) {
    let made = create_instance(site, api, name).await;
    let summary = api.start_ready(made.id).await;
    let sentinel =
        SentinelCluster::new(name, made.id, site.psql(made.id, made.port), &made.password);
    let value = format!("sentinel of {name}");
    sentinel.write(&value);
    (
        Cluster {
            made,
            sentinel,
            value,
        },
        summary,
    )
}

fn assert_every_sentinel_intact(clusters: &[Cluster]) {
    for cluster in clusters {
        SentinelCluster::assert_isolated(&[(&cluster.sentinel, &cluster.value)]);
    }
}

/// The processes that hold `lock` and have no parent among the other
/// holders: one per postmaster.
fn postmasters_of(lock: &Path) -> Vec<u32> {
    let holders: Vec<u32> = output_of("/usr/sbin/lsof", &["-t", &lock.display().to_string()])
        .split_whitespace()
        .filter_map(|pid| pid.parse().ok())
        .collect();
    holders
        .iter()
        .copied()
        .filter(|pid| {
            let parent: u32 = output_of("/bin/ps", &["-o", "ppid=", "-p", &pid.to_string()])
                .trim()
                .parse()
                .unwrap_or(0);
            !holders.contains(&parent)
        })
        .collect()
}

/// The names of the files under `dir`, relative to it.
fn names_under(dir: &Path) -> BTreeSet<String> {
    tree_digest(dir).into_keys().collect()
}

async fn closing_every_client_keeps_three_instances_unchanged(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.binary(&[]);
    let api = site.binary_api(&daemon).await;
    let mut clusters = Vec::new();
    let mut before = Vec::new();
    for name in ["close-a", "close-b", "close-c"] {
        let (cluster, summary) = running_cluster(&site, &api, name).await;
        clusters.push(cluster);
        before.push(summary);
    }
    let version = clusters[0]
        .sentinel
        .psql
        .scalar(&clusters[0].made.password, "SHOW server_version");
    site.subject(&ev, &version);
    let (first, accepted) = common::client(&daemon.socket).await;
    let server_before = accepted.server_instance_id;

    // Every client of the daemon goes away, as when the app closes.
    drop(first);
    drop(api);
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert!(
        std::process::Command::new("/bin/kill")
            .args(["-0", &daemon.pid().to_string()])
            .status()
            .expect("kill runs")
            .success(),
        "the daemon outlives its clients"
    );

    // The app opens again and takes the state from the daemon.
    let (second, accepted) = common::client(&daemon.socket).await;
    assert_eq!(accepted.server_instance_id, server_before);
    drop(second);
    let api = site.binary_api(&daemon).await;
    for (summary, cluster) in before.iter().zip(&clusters) {
        let now = api
            .get(cluster.made.id)
            .await
            .expect("the instance can be read");
        let (then, now_process) = (
            summary.process.as_ref().expect("a process before"),
            now.process.as_ref().expect("a process after"),
        );
        assert_eq!(now.id, summary.id);
        assert_eq!(now_process.pid, then.pid);
        assert_eq!(now_process.started_at, then.started_at);
        assert!(!now_process.adopted, "the daemon never lost it");
        assert_eq!(now.lifecycle, InstanceLifecycle::Running);
        assert_eq!(now.readiness.state, InstanceReadinessState::Ready);
        assert_eq!(
            cluster
                .sentinel
                .psql
                .scalar(&cluster.made.password, "SELECT 1"),
            "1"
        );
    }
    assert_every_sentinel_intact(&clusters);

    for cluster in &clusters {
        let accepted = api.stop_when_free(cluster.made.id).await.accepted;
        let accepted = accepted.expect("a stop");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    }
    daemon.stop();
    ev.pass_with(&format!(
        "PostgreSQL {version}: three instances kept pid, start time, and id after every client closed and the app asked again"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn closing_every_client_keeps_three_postgresql_instances_unchanged() {
    // covers: spec 0030 AC-1 (native part; the manual record is milestone 6)
    let ev = evidence!(
        covers = ["REQ-001", "AC-001"],
        kind = Kind::Native,
        expected = "with three PostgreSQL instances ready, every client of the daemon closes and opens again, and each instance keeps its id, pid, and start time, answers SELECT 1, and reads its own sentinel",
    );
    closing_every_client_keeps_three_instances_unchanged(PG17, ev).await;
}

async fn sigkill_leaves_one_postmaster_per_data_folder(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let first = site.binary(&[]);
    let api = site.binary_api(&first).await;
    let mut clusters = Vec::new();
    let mut before = Vec::new();
    for name in ["kill-a", "kill-b", "kill-c"] {
        let (cluster, summary) = running_cluster(&site, &api, name).await;
        clusters.push(cluster);
        before.push(summary);
    }
    let version = clusters[0]
        .sentinel
        .psql
        .scalar(&clusters[0].made.password, "SHOW server_version");
    site.subject(&ev, &version);
    // A fourth instance that was stopped when the daemon died.
    let idle = create_instance(&site, &api, "kill-idle").await;
    let idle_data = site.instance_paths(idle.id).data;
    let idle_before = tree_digest(&idle_data);
    let idle_summary = api.get(idle.id).await.expect("the idle instance reads");
    assert_eq!(idle_summary.lifecycle, InstanceLifecycle::Stopped);
    let data_names: Vec<_> = clusters
        .iter()
        .map(|cluster| names_under(&site.instance_paths(cluster.made.id).data))
        .collect();

    drop(api);
    first.kill();

    let second = site.binary(&[]);
    let api = site.binary_api(&second).await;
    for (summary, cluster) in before.iter().zip(&clusters) {
        let adopted = api
            .wait_instance(cluster.made.id, "adopted and ready", |now| {
                now.process.as_ref().is_some_and(|process| process.adopted)
                    && now.readiness.state == InstanceReadinessState::Ready
            })
            .await;
        let (then, now) = (
            summary.process.as_ref().expect("a process before"),
            adopted.process.as_ref().expect("a process after"),
        );
        assert_eq!(now.pid, then.pid);
        assert_eq!(now.started_at, then.started_at);
        assert_eq!(adopted.lifecycle, InstanceLifecycle::Running);
        let lock = site.instance_paths(cluster.made.id).lock;
        assert_eq!(
            postmasters_of(&lock),
            vec![then.pid],
            "exactly one postmaster holds the data of {}",
            cluster.sentinel.name
        );
    }
    assert_every_sentinel_intact(&clusters);
    let idle_now = api.get(idle.id).await.expect("the idle instance reads");
    assert_eq!(idle_now.lifecycle, InstanceLifecycle::Stopped);
    assert!(idle_now.process.is_none());
    assert_eq!(
        tree_digest(&idle_data),
        idle_before,
        "the data of a stopped instance is untouched"
    );
    for (cluster, names) in clusters.iter().zip(&data_names) {
        let now = names_under(&site.instance_paths(cluster.made.id).data);
        let lost: Vec<_> = names.difference(&now).collect();
        // The server recycles WAL segments and temporary files itself;
        // nothing else leaves the folder.
        assert!(
            lost.iter().all(|name| name.starts_with("pg_wal/")
                || name.starts_with("pg_stat_tmp/")
                || name.starts_with("pg_logical/")
                || name.starts_with("pg_notify/")),
            "files left the data of {}: {lost:?}",
            cluster.sentinel.name
        );
    }

    for cluster in &clusters {
        let accepted = api.stop_when_free(cluster.made.id).await.accepted;
        let accepted = accepted.expect("a stop");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
    }
    second.stop();
    ev.pass_with(&format!(
        "PostgreSQL {version}: three clusters adopted with the same pid and start time after SIGKILL of the daemon, one postmaster per data folder, sentinels intact, the stopped instance untouched"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn sigkill_leaves_one_postmaster_per_data_folder_and_every_sentinel_intact() {
    // covers: spec 0030 AC-2
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Native,
        expected = "after SIGKILL of the daemon, three running PostgreSQL clusters are adopted with the same pid and start time, exactly one postmaster holds each data folder, every sentinel reads back, and the stopped instance stays stopped with its data unchanged",
    );
    sigkill_leaves_one_postmaster_per_data_folder(PG18, ev).await;
}

/// How the data of a stopped instance goes missing.
#[derive(Clone, Copy, Debug)]
enum Loss {
    Folder,
    Emptied,
    Marker,
}

impl Loss {
    fn apply(self, data: &Path) {
        match self {
            Self::Folder => std::fs::remove_dir_all(data).expect("the data folder is deleted"),
            Self::Emptied => {
                for entry in std::fs::read_dir(data).expect("the data folder reads") {
                    let path = entry.expect("an entry reads").path();
                    if path.is_dir() {
                        std::fs::remove_dir_all(path).expect("a folder is deleted");
                    } else {
                        std::fs::remove_file(path).expect("a file is deleted");
                    }
                }
            }
            Self::Marker => {
                std::fs::remove_file(data.join("PG_VERSION")).expect("the data marker is deleted")
            }
        }
    }
}

async fn missing_data_fails_without_init_or_new_folders(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let first = site.binary(&[]);
    let api = site.binary_api(&first).await;
    let mut lost = Vec::new();
    for (name, loss) in [
        ("gone-folder", Loss::Folder),
        ("gone-empty", Loss::Emptied),
        ("gone-marker", Loss::Marker),
    ] {
        let made = create_instance(&site, &api, name).await;
        let summary = api.start_ready(made.id).await;
        let version = site
            .psql(made.id, made.port)
            .scalar(&made.password, "SHOW server_version");
        site.subject(&ev, &version);
        let accepted = api.stop_when_free(made.id).await.accepted;
        let accepted = accepted.expect("a stop");
        api.wait_state(accepted.operation.id, OperationState::Succeeded)
            .await;
        api.wait_instance(made.id, "stopped", |now| {
            now.lifecycle == InstanceLifecycle::Stopped
        })
        .await;
        lost.push((made.id, name, loss, summary.id));
    }
    drop(api);
    first.stop();

    let mut kept = Vec::new();
    let mut run_dirs = Vec::new();
    for (id, _, loss, _) in &lost {
        let data = site.instance_paths(*id).data;
        loss.apply(&data);
        kept.push(names_under(&data));
        run_dirs.push(site.run_dir(*id).exists());
    }

    let second = site.binary(&[]);
    let api = site.binary_api(&second).await;
    for (((id, name, loss, _), names), run_dir_before) in lost.iter().zip(&kept).zip(&run_dirs) {
        let failed = api
            .wait_instance(*id, "failed with data_missing", |now| {
                now.lifecycle == InstanceLifecycle::Failed
            })
            .await;
        assert_eq!(
            failed
                .last_failure
                .as_ref()
                .map(|failure| failure.code.as_str()),
            Some("data_missing"),
            "{name}"
        );
        assert!(failed.process.is_none(), "{name}");
        let accepted = api
            .start(*id)
            .await
            .expect("the start is accepted")
            .accepted
            .expect("a start from failed begins an operation");
        let detail = api.wait_final(accepted.operation.id).await;
        assert_eq!(detail.operation.state, OperationState::Failed, "{name}");
        assert_eq!(
            Api::failure_code(&detail),
            "supervisor.data_unavailable",
            "{name}"
        );
        let data = site.instance_paths(*id).data;
        // Nothing was made: no data folder, no new file, no run folder.
        match loss {
            Loss::Folder => assert!(!data.exists(), "{name}: the folder came back"),
            _ => assert_eq!(&names_under(&data), names, "{name}: the data changed"),
        }
        assert_eq!(
            site.run_dir(*id).exists(),
            *run_dir_before,
            "{name}: a run folder was made or lost"
        );
    }
    let processes = output_of(
        "/usr/bin/pgrep",
        &["-f", &site.root.path().display().to_string()],
    );
    let running: Vec<_> = processes
        .split_whitespace()
        .filter(|pid| pid.parse::<u32>().ok() != Some(second.pid()))
        .filter(|pid| pid.parse::<u32>().ok() != Some(std::process::id()))
        .collect();
    assert!(
        running.iter().all(|pid| {
            !output_of("/bin/ps", &["-o", "command=", "-p", pid]).contains("postgres")
        }),
        "no postgres runs for the lost data: {running:?}"
    );
    second.stop();
    ev.pass_with("three stopped PostgreSQL instances whose data folder, contents, or marker was deleted failed with data_missing, refused a start with data_unavailable, and no folder or file was made");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn missing_data_fails_the_instance_without_init_start_or_new_folders() {
    // covers: spec 0030 AC-4 (native part)
    let ev = evidence!(
        covers = ["REQ-013", "AC-013"],
        kind = Kind::Native,
        expected = "a stopped PostgreSQL instance whose data folder is deleted, emptied, or loses its marker fails with data_missing after the daemon restarts, refuses a start with supervisor.data_unavailable, and the daemon makes no data or run folder and starts no postgres",
    );
    missing_data_fails_without_init_or_new_folders(PG17, ev).await;
}
