//! AC-10: a closed stdout is silent and keeps the exit code; another write
//! failure is one `cli.output_failed` line on stderr.

use std::fs::File;
use std::io::Write;
use std::process::Stdio;

use stackly_protocol::Reply;
use stackly_protocol::daemon::DaemonHealthStatus;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{DiskImage, TestRoot};

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{dto, stackly};

fn ready(root: &TestRoot) -> ScriptedDaemon {
    ScriptedDaemon::serve(root, |call, _| match call.method() {
        "daemon.get_info" => vec![Step::Reply(Reply::DaemonGetInfo(dto::info(false)))],
        _ => vec![Step::Reply(Reply::DaemonGetHealth(dto::health(
            DaemonHealthStatus::Ready,
        )))],
    })
}

#[test]
fn a_reader_that_went_away_ends_the_output_silently_with_the_commands_exit_code() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "when the reader of stdout closed before the result is written, stackly writes nothing to stderr and still exits 0, in text and --json",
    );
    let root = TestRoot::new().unwrap();
    let _daemon = ready(&root);

    for json in [false, true] {
        let mut command = stackly(root.path());
        command
            .args(["daemon", "status"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if json {
            command.arg("--json");
        }
        let mut child = command.spawn().unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();

        assert_eq!(output.status.code(), Some(0), "json {json}: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "", "json {json}");
    }
    ev.pass();
}

/// Fills the volume at `dir` until a write fails for lack of space.
fn fill(dir: &std::path::Path) {
    let mut filler = File::create(dir.join("filler")).expect("the image takes a file");
    for size in [64 * 1024, 4096, 512, 1] {
        let block = vec![0_u8; size];
        loop {
            match filler.write_all(&block).and_then(|()| filler.sync_all()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::StorageFull => break,
                Err(error) => panic!("filling the image failed: {error}"),
            }
        }
    }
}

#[test]
fn a_stdout_that_cannot_be_written_gives_one_warning_line_and_keeps_the_exit_code() {
    let root = TestRoot::new().unwrap();
    let _daemon = ready(&root);
    let image = DiskImage::attach(&root, &root.path().join("full")).unwrap();
    let outputs: Vec<std::path::PathBuf> = (0..2)
        .map(|n| {
            let path = image.mount_point().join(format!("stdout-{n}"));
            File::create(&path).unwrap();
            path
        })
        .collect();
    fill(image.mount_point());
    let mut probe = std::fs::OpenOptions::new()
        .append(true)
        .open(&outputs[0])
        .unwrap();
    assert_eq!(
        probe
            .write_all(&[b'x'; 2048])
            .and_then(|()| probe.sync_all())
            .map_err(|error| error.kind()),
        Err(std::io::ErrorKind::StorageFull),
        "the image is full"
    );

    for (json, path) in [false, true].into_iter().zip(&outputs) {
        let stdout = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        let mut command = stackly(root.path());
        command
            .args(["daemon", "status"])
            .stdout(stdout)
            .stderr(Stdio::piped());
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();

        assert_eq!(output.status.code(), Some(0), "json {json}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
        assert!(
            stderr.starts_with("warning: cli.output_failed: cannot write the result to stdout"),
            "{stderr}"
        );
    }
}
