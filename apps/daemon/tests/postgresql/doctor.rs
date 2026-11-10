//! The doctor on a real PostgreSQL that refuses an old `postmaster.pid`
//! (spec 0034 AC-2, AC-10, AC-12, AC-13): the file names a live `sleep` of
//! this test, so `postgres` refuses to start, and the doctor reports
//! `start_failed` with the `stale_postmaster_pid` remedy of the package and
//! what the PID is, while the file, the data, and the `sleep` stay exactly
//! as they were. A bundle preview and its export run the same doctor and
//! leave `postmaster.pid` byte for byte and with its time of change.

use std::path::Path;
use std::process::{Child, Command};

use sha2::{Digest, Sha256};
use stackly_protocol::diagnostics::ExportBundleParams;
use stackly_protocol::doctor::{DoctorCheckId, DoctorFindingCode, DoctorParams, PidFileState};
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance};

/// A `sleep` of this test, killed and reaped on drop.
struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn an_old_postmaster_pid_gets_the_provider_remedy_and_stays(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "lockfile").await;
    let sleeper = Sleeper(
        Command::new("/bin/sleep")
            .arg("600")
            .spawn()
            .expect("sleep starts"),
    );
    let pid = sleeper.0.id();
    let data = site.instance_paths(made.id).data;
    let pid_file = data.join("postmaster.pid");
    let text = format!("{pid}\n{}\n", data.display());
    std::fs::write(&pid_file, &text).expect("the old postmaster.pid is written");

    let operation = api
        .start(made.id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start of a stopped instance begins an operation")
        .operation
        .id;
    api.wait_final(operation).await;
    let failed = api.get(made.id).await.expect("the instance reads");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed, "{failed:?}");
    assert_eq!(failed.runtime_version, pg.version);
    site.subject(&ev, pg.version);

    let reply = api
        .client
        .call(Call::DoctorRun(DoctorParams {
            instance_id: Some(made.id),
            checks: vec![DoctorCheckId::InstanceFailures],
        }))
        .await
        .expect("doctor.run answers");
    let Reply::DoctorRun(report) = reply else {
        panic!("not a doctor reply: {reply:?}");
    };
    let finding = report.checks[0]
        .findings
        .iter()
        .find(|finding| finding.code == DoctorFindingCode::StartFailed)
        .expect("start_failed is reported");
    let remedy = finding
        .remedy
        .as_ref()
        .expect("the stale postmaster.pid remedy matches");
    assert_eq!(remedy.provider, "official/postgresql");
    assert_eq!(remedy.name, "stale_postmaster_pid");
    let evidence = remedy.pid_file.as_ref().expect("the PID file is read");
    assert_eq!(evidence.pid, Some(pid));
    assert_eq!(evidence.state, PidFileState::OtherProcess);
    assert_eq!(
        evidence
            .process
            .as_ref()
            .and_then(|process| process.executable.as_deref()),
        Some("/bin/sleep")
    );
    let sentence = format!("PID {pid} now belongs to /bin/sleep, not PostgreSQL.");
    assert!(
        finding.explanation.ends_with(&sentence),
        "{}",
        finding.explanation
    );
    assert!(
        finding
            .excerpt
            .iter()
            .any(|line| line.contains("lock file \"postmaster.pid\" already exists")),
        "{:?}",
        finding.excerpt
    );

    assert_eq!(
        std::fs::read_to_string(&pid_file).expect("postmaster.pid stays"),
        text,
        "the doctor never changes or removes postmaster.pid"
    );
    assert!(
        Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .status()
            .expect("kill -0 runs")
            .success(),
        "the sleep still runs"
    );
    drop(sleeper);
    daemon.shutdown().await;
    ev.pass_with(&format!("start_failed with {} and {sentence}", remedy.name));
}

// covers: AC-2, AC-10, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_with_an_old_postmaster_pid_gets_the_stale_postmaster_pid_remedy() {
    let ev = evidence!(
        covers = ["REQ-019", "AC-019"],
        kind = Kind::Native,
        expected = "a postmaster.pid naming a live sleep makes PostgreSQL 18 refuse to start; doctor.run reports start_failed with the stale_postmaster_pid remedy, the PID as other_process /bin/sleep, and the lock file excerpt, and postmaster.pid and the sleep stay",
    );
    an_old_postmaster_pid_gets_the_provider_remedy_and_stays(PG18, ev).await;
}

// covers: AC-2, AC-10, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_with_an_old_postmaster_pid_gets_the_stale_postmaster_pid_remedy() {
    let ev = evidence!(
        covers = ["REQ-019", "AC-019"],
        kind = Kind::Native,
        expected = "a postmaster.pid naming a live sleep makes PostgreSQL 17 refuse to start; doctor.run reports start_failed with the stale_postmaster_pid remedy, the PID as other_process /bin/sleep, and the lock file excerpt, and postmaster.pid and the sleep stay",
    );
    an_old_postmaster_pid_gets_the_provider_remedy_and_stays(PG17, ev).await;
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The SHA-256 and time of change of every regular file under `folder`, by
/// relative path.
fn files(folder: &Path) -> Vec<(String, String, std::time::SystemTime)> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = entry.file_type().expect("the entry has a type");
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let bytes = std::fs::read(&path).expect("a file reads");
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .expect("a file has a time of change");
                let relative = path
                    .strip_prefix(folder)
                    .expect("under the folder")
                    .display()
                    .to_string();
                found.push((relative, sha256(&bytes), modified));
            }
        }
    }
    found.sort();
    found
}

async fn a_bundle_of_an_old_postmaster_pid_reports_the_remedy_and_leaves_the_file(
    pg: Pg,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "bundled").await;
    let sleeper = Sleeper(
        Command::new("/bin/sleep")
            .arg("600")
            .spawn()
            .expect("sleep starts"),
    );
    let pid = sleeper.0.id();
    let paths = site.instance_paths(made.id);
    let pid_file = paths.data.join("postmaster.pid");
    let text = format!("{pid}\n{}\n", paths.data.display());
    std::fs::write(&pid_file, &text).expect("the old postmaster.pid is written");
    let operation = api
        .start(made.id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start of a stopped instance begins an operation")
        .operation
        .id;
    api.wait_final(operation).await;
    let failed = api.get(made.id).await.expect("the instance reads");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed, "{failed:?}");
    assert_eq!(failed.runtime_version, pg.version);
    site.subject(&ev, pg.version);

    let pid_modified = std::fs::metadata(&pid_file)
        .and_then(|metadata| metadata.modified())
        .expect("postmaster.pid has a time of change");
    let data_before = files(&paths.data);
    let lock_before = std::fs::read(&paths.lock).expect("instance.lock reads");

    let reply = api
        .client
        .call(Call::DiagnosticsPreviewBundle)
        .await
        .expect("diagnostics.preview_bundle answers");
    let Reply::DiagnosticsPreviewBundle(preview) = reply else {
        panic!("not a preview reply: {reply:?}");
    };
    let out = site.root.path().join("out");
    std::fs::create_dir(&out).expect("the export folder can be made");
    let target = out.join(&preview.suggested_file_name);
    let reply = api
        .client
        .call(Call::DiagnosticsExportBundle(ExportBundleParams {
            preview_id: preview.preview_id,
            destination: target.display().to_string(),
            replace: false,
        }))
        .await
        .expect("diagnostics.export_bundle answers");
    let Reply::DiagnosticsExportBundle(exported) = reply else {
        panic!("not an export reply: {reply:?}");
    };
    let zip = std::fs::read(&target).expect("the bundle is written");
    assert_eq!(exported.sha256, sha256(&zip));
    let entries = stackly_core::diagnostics::testing::unzip(&zip);
    let doctor = String::from_utf8_lossy(&entries["doctor.json"]).into_owned();
    assert!(
        doctor.contains("start_failed") && doctor.contains("stale_postmaster_pid"),
        "the doctor of the bundle reports the failure with its remedy: {doctor}"
    );
    assert!(
        doctor.contains(&format!(
            "PID {pid} now belongs to /bin/sleep, not PostgreSQL."
        )),
        "{doctor}"
    );

    assert_eq!(
        std::fs::read_to_string(&pid_file).expect("postmaster.pid stays"),
        text,
        "the bundle never changes or removes postmaster.pid"
    );
    assert_eq!(
        std::fs::metadata(&pid_file)
            .and_then(|metadata| metadata.modified())
            .expect("postmaster.pid has a time of change"),
        pid_modified,
        "and never touches it"
    );
    let data_after = files(&paths.data);
    assert_eq!(data_after, data_before, "every data file stays");
    assert_eq!(
        std::fs::read(&paths.lock).expect("instance.lock reads"),
        lock_before
    );
    assert!(
        Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .status()
            .expect("kill -0 runs")
            .success(),
        "the sleep still runs"
    );
    let after = api.get(made.id).await.expect("the instance reads");
    assert_eq!(
        after.revision, failed.revision,
        "the instance is not written"
    );
    assert_eq!(after.lifecycle, InstanceLifecycle::Failed);
    drop(sleeper);
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "{} data files, postmaster.pid, and instance.lock unchanged; doctor.json has stale_postmaster_pid",
        data_after.len()
    ));
}

// covers: AC-2, AC-12, AC-13, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_keeps_an_old_postmaster_pid_through_a_bundle_preview_and_export() {
    let ev = evidence!(
        covers = ["REQ-019", "AC-019", "NFR-016"],
        kind = Kind::Native,
        expected = "spec 0034 AC-2: with PostgreSQL 18 failed on a postmaster.pid naming a live sleep, diagnostics.preview_bundle and diagnostics.export_bundle write a bundle whose doctor.json reports start_failed with stale_postmaster_pid, and postmaster.pid (bytes and time of change), every data file, instance.lock, the sleep, and the instance revision stay",
    );
    a_bundle_of_an_old_postmaster_pid_reports_the_remedy_and_leaves_the_file(PG18, ev).await;
}

// covers: AC-2, AC-12, AC-13, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_keeps_an_old_postmaster_pid_through_a_bundle_preview_and_export() {
    let ev = evidence!(
        covers = ["REQ-019", "AC-019", "NFR-016"],
        kind = Kind::Native,
        expected = "spec 0034 AC-2: with PostgreSQL 17 failed on a postmaster.pid naming a live sleep, diagnostics.preview_bundle and diagnostics.export_bundle write a bundle whose doctor.json reports start_failed with stale_postmaster_pid, and postmaster.pid (bytes and time of change), every data file, instance.lock, the sleep, and the instance revision stay",
    );
    a_bundle_of_an_old_postmaster_pid_reports_the_remedy_and_leaves_the_file(PG17, ev).await;
}
