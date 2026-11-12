//! The init of a MySQL instance (spec 0042 AC-3, BR-008): one
//! `mysqld --initialize-insecure` that opens no port and no socket, leaves
//! no password file, no process, and no binary log, and fails closed on a
//! bad account statement or a data folder that already holds a file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use stackly_core::instance::create::Hook;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};

use crate::harness::{
    MY84, My, Site, binlog_files, create_generated, failed_create, listeners, mysqld_serving,
    new_password, output_of, package_copy_with,
};
use crate::support::LogRecorder;

/// Where the steps of `providers/mysql` sit in its init plan: the folders
/// (0), the password file (1), and `mysqld --initialize-insecure` (2).
pub const INITIALIZE_STEP: &str = "2";

/// What a watcher saw of the `mysqld --initialize-insecure` processes under
/// one test root.
#[derive(Debug, Default)]
struct Seen {
    pids: Vec<u32>,
    listening: Vec<String>,
    sockets: Vec<String>,
}

/// Polls every 20 ms for an `--initialize-insecure` process under `root` and
/// records what it listens on, until `stop` is set.
fn watch_init(root: PathBuf, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<Seen> {
    std::thread::spawn(move || {
        let mut seen = Seen::default();
        let needle = root.display().to_string();
        while !stop.load(Ordering::SeqCst) {
            for line in output_of("/bin/ps", &["-axww", "-o", "pid=,command="]).lines() {
                if !(line.contains("--initialize-insecure") && line.contains(&needle)) {
                    continue;
                }
                let Some(pid) = line
                    .split_whitespace()
                    .next()
                    .and_then(|pid| pid.parse::<u32>().ok())
                else {
                    continue;
                };
                if !seen.pids.contains(&pid) {
                    seen.pids.push(pid);
                }
                let (listening, sockets) = listeners(pid);
                seen.listening.extend(listening);
                seen.sockets.extend(sockets);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        seen
    })
}

async fn initializes_in_one_process_that_opens_nothing_and_leaves_nothing(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    let stop = Arc::new(AtomicBool::new(false));
    let watcher = watch_init(site.root.path().to_path_buf(), Arc::clone(&stop));
    let made = create_generated(&site, &api, "init").await;
    stop.store(true, Ordering::SeqCst);
    let seen = watcher.join().expect("the watcher ends");

    assert_eq!(seen.pids.len(), 1, "one initialize process: {seen:?}");
    assert!(seen.listening.is_empty(), "init listened: {seen:?}");
    assert!(seen.sockets.is_empty(), "init opened a socket: {seen:?}");
    let paths = site.instance_paths(made.id);
    assert!(
        !site.run_dir(made.id).join("secrets/root-password").exists(),
        "the password file outlived the init"
    );
    assert!(
        mysqld_serving(&paths.data).is_empty(),
        "a mysqld outlived the init"
    );
    assert!(!binlog_files(&paths.data), "init wrote a binary log");
    assert!(paths.data.join("mysql_upgrade_history").is_file());
    assert!(!site.run_dir(made.id).join("mysql.sock").exists());

    daemon.shutdown().await;
    ev.subject(
        "mysql",
        my.version,
        Some(&site.artifact_sha256),
        Source::Managed,
    );
    ev.pass_with("one initialize process, no listener, no socket, no password file, no binlog");
}

// covers: AC-3 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_initializes_in_one_process_that_opens_nothing_and_leaves_nothing() {
    let ev = evidence!(
        covers = ["REQ-055", "BR-008"],
        kind = Kind::Native,
        expected = "the init runs exactly one mysqld --initialize-insecure, which listens on no TCP port and opens no Unix socket, and afterwards no root-password file, no mysqld process, and no binlog.* file is left while mysql_upgrade_history is written",
    );
    initializes_in_one_process_that_opens_nothing_and_leaves_nothing(MY84, ev).await;
}

/// The error of a failed operation, with the reason, step, and exit code it
/// names.
fn init_failure(
    detail: &stackly_protocol::operation::OperationDetail,
) -> (String, String, String, String) {
    let error = &detail
        .operation
        .failure
        .as_ref()
        .expect("a failed operation names its error")
        .error;
    let text = |key: &str| error.detail(key).unwrap_or_default().to_owned();
    (
        error.code().to_owned(),
        text("reason"),
        text("step"),
        text("exitCode"),
    )
}

async fn fails_closed_on_a_bad_account_statement_and_never_starts(my: My, ev: Evidence) {
    let copies = TestRoot::new().expect("a folder for the package copy");
    // `root@localhost` exists after --initialize, so creating it again fails.
    let dir = package_copy_with(
        &copies,
        "CREATE USER 'stackly_readiness'@'localhost' IDENTIFIED BY '' WITH",
        "CREATE USER 'root'@'localhost' IDENTIFIED BY '' WITH",
    );
    let Some(site) = Site::open_package(&ev, my, &dir) else {
        return;
    };
    let recorder = Arc::new(LogRecorder::default());
    let daemon = site
        .in_process(|config| {
            config
                .extra_log_sinks
                .push(Arc::clone(&recorder) as Arc<dyn stackly_core::supervisor::LogSink>);
        })
        .await;
    let api = site.api().await;
    let password = new_password();

    let (id, detail) = failed_create(&site, &api, "badsql", &password).await;

    assert_eq!(
        init_failure(&detail),
        (
            "supervisor.init_failed".to_owned(),
            "init_step_failed".to_owned(),
            INITIALIZE_STEP.to_owned(),
            "1".to_owned()
        ),
        "{detail:?}"
    );
    let lines = recorder.text_of(id);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Operation CREATE USER failed for 'root'@'localhost'")),
        "the MySQL error reached the log sinks: {lines:#?}"
    );
    assert!(lines.iter().all(|line| !line.contains(&password)));
    assert!(
        !site.run_dir(id).join("secrets/root-password").exists(),
        "the password file outlived the failed init"
    );
    assert!(mysqld_serving(&site.instance_paths(id).data).is_empty());
    let gone = api
        .get(id)
        .await
        .expect_err("the instance of a failed create is gone");
    assert_eq!(gone.code(), "resource.not_found", "{gone:?}");
    daemon.shutdown().await;
    ev.subject(
        "mysql",
        my.version,
        Some(&site.artifact_sha256),
        Source::Managed,
    );
    ev.pass_with("mysqld exited 1 at the account SQL, init_step_failed, nothing started");
}

// covers: AC-3 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_fails_closed_on_a_bad_account_statement_and_never_starts() {
    let ev = evidence!(
        covers = ["REQ-055", "BR-008"],
        kind = Kind::Native,
        expected = "a copy of the package whose account SQL creates root@localhost again makes mysqld --initialize-insecure exit 1, the init fails init_step_failed at the initialize step with the MySQL error in the log sinks, no password file or mysqld is left, and the create is compensated",
    );
    fails_closed_on_a_bad_account_statement_and_never_starts(MY84, ev).await;
}

/// The bytes and modification time of `path`.
fn state_of(path: &Path) -> (Vec<u8>, SystemTime) {
    (
        std::fs::read(path).expect("the file can be read"),
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .expect("the file has a modification time"),
    )
}

async fn refuses_a_data_folder_that_holds_a_file_and_leaves_the_file_alone(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    // The create refuses a folder that is not empty before init starts
    // (`instance.data_not_empty`), so the file goes in after that check,
    // through the hook of the create. It is a hard link to a file outside
    // the instance, which outlives the compensation of the create.
    let outside = TestRoot::new().expect("a folder for the outside file");
    let original = outside.path().join("keep.txt");
    std::fs::write(&original, b"keep me").expect("the outside file is written");
    let before = state_of(&original);
    let linked = Arc::new(Mutex::new(false));
    let hook_original = original.clone();
    let hook_linked = Arc::clone(&linked);
    let stray: Hook = Arc::new(move |paths| {
        std::fs::hard_link(&hook_original, paths.data.join("keep.txt"))
            .expect("the data folder takes the link");
        *hook_linked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    });
    let recorder = Arc::new(LogRecorder::default());
    let daemon = site
        .in_process(|config| {
            config.create_hooks.before_init = Some(stray);
            config
                .extra_log_sinks
                .push(Arc::clone(&recorder) as Arc<dyn stackly_core::supervisor::LogSink>);
        })
        .await;
    let api = site.api().await;

    let (id, detail) = failed_create(&site, &api, "notempty", &new_password()).await;

    assert!(
        *linked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    );
    assert_eq!(
        init_failure(&detail),
        (
            "supervisor.init_failed".to_owned(),
            "init_step_failed".to_owned(),
            INITIALIZE_STEP.to_owned(),
            "1".to_owned()
        ),
        "{detail:?}"
    );
    let lines = recorder.text_of(id);
    assert!(
        lines.iter().any(|line| line.contains("has files in it")),
        "the MySQL error reached the log sinks: {lines:#?}"
    );
    assert_eq!(state_of(&original), before, "the file was changed");
    assert!(!site.instance_paths(id).root.exists());
    daemon.shutdown().await;
    ev.subject(
        "mysql",
        my.version,
        Some(&site.artifact_sha256),
        Source::Managed,
    );
    ev.pass_with("mysqld refused the data folder with exit 1 and the file kept its bytes");
}

// covers: AC-3 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_refuses_a_data_folder_that_holds_a_file_and_leaves_the_file_alone() {
    let ev = evidence!(
        covers = ["REQ-055", "BR-008"],
        kind = Kind::Native,
        expected = "a file put in the data folder after the create checked it empty makes mysqld --initialize-insecure exit 1, the init fails init_step_failed at the initialize step, and the file keeps its bytes and modification time",
    );
    refuses_a_data_folder_that_holds_a_file_and_leaves_the_file_alone(MY84, ev).await;
}
