//! Every path of a running `mysqld` stays in its instance (spec 0042 AC-6):
//! the path variables, the files it opens for writing, and the error log on
//! stderr that `instance.logs_read` returns.

use std::path::{Path, PathBuf};

use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::config_calls::pid_of;
use crate::harness::{MY84, My, Site, create_instance, output_of, stop_and_shutdown, variable};

/// The regular files `pid` has open for writing: the `n` field of each
/// numbered fd whose access mode is `w` or `u` and whose type is `REG`.
fn files_open_for_writing(pid: u32) -> Vec<PathBuf> {
    let text = output_of(
        "/usr/sbin/lsof",
        &["-a", "-p", &pid.to_string(), "-n", "-P", "-F", "fatn"],
    );
    let mut found = Vec::new();
    let (mut numbered, mut writing, mut regular) = (false, false, false);
    for line in text.lines() {
        let (tag, value) = line.split_at(1);
        match tag {
            "f" => {
                numbered = value.chars().all(|c| c.is_ascii_digit());
                writing = false;
                regular = false;
            }
            "a" => writing = matches!(value, "w" | "u"),
            "t" => regular = value == "REG",
            "n" if numbered && writing && regular => found.push(PathBuf::from(value)),
            _ => {}
        }
    }
    found
}

/// `path` with `/private` taken off the front, the way `lsof` and MySQL
/// may show a path under `/tmp` or `/var`.
fn plain(path: &Path) -> PathBuf {
    path.strip_prefix("/private")
        .map_or_else(|_| path.to_path_buf(), |rest| Path::new("/").join(rest))
}

async fn keeps_every_path_in_its_instance(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "paths").await;
    let running = api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);
    let paths = site.instance_paths(made.id);
    let run = site.run_dir(made.id);
    let read = |name: &str| PathBuf::from(variable(&mysql, &made.password, name));
    let under = |path: &Path, dir: &Path| plain(path).starts_with(plain(dir));

    assert!(
        under(&read("datadir"), &paths.data),
        "{:?}",
        read("datadir")
    );
    assert_eq!(plain(&read("tmpdir")), plain(&paths.temp));
    for name in ["general_log_file", "slow_query_log_file"] {
        assert!(under(&read(name), &paths.logs), "{name}: {:?}", read(name));
    }
    for name in ["pid_file", "socket"] {
        assert!(under(&read(name), &run), "{name}: {:?}", read(name));
    }
    assert_eq!(variable(&mysql, &made.password, "secure_file_priv"), "NULL");
    assert_eq!(variable(&mysql, &made.password, "log_error"), "stderr");

    let written = files_open_for_writing(pid_of(&running));
    assert!(!written.is_empty(), "mysqld writes its data files");
    // The instance lock is Stackly's, inherited at fd 3 (spec 0010); the
    // process does not open it itself.
    for file in written
        .iter()
        .filter(|file| plain(file) != plain(&paths.lock))
    {
        assert!(
            [&paths.data, &paths.temp, &paths.logs, &run]
                .iter()
                .any(|dir| under(file, dir)),
            "{} is outside the instance",
            file.display()
        );
    }

    let lines = crate::harness::log_lines(&api, made.id).await;
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[System]") && line.contains("ready for connections")),
        "{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains('T') && line.contains('Z')),
        "an error log line carries its UTC stamp: {lines:#?}"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "path variables and {} files open for writing are in the instance, and the error log reached instance.logs_read",
        written.len()
    ));
}

// covers: AC-6 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_keeps_every_path_in_its_instance() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "@@datadir is in the data folder, @@tmpdir is the temp folder, the general and slow log files are in the logs folder, the pid file and socket are in the run folder, secure_file_priv is NULL and log_error is stderr, every regular file mysqld has open for writing is in one of those folders, and the error log lines with their UTC stamp come back from instance.logs_read",
    );
    keeps_every_path_in_its_instance(MY84, ev).await;
}
