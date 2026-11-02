//! The psql Open In script against a real PostgreSQL (spec 0070 AC-14,
//! AC-15). The script `instance.prepare_terminal` writes runs in a
//! pseudo terminal made by `/usr/bin/script`, with no GUI app opened:
//! psql asks for the password, the pasted password runs `SELECT 1`, no
//! process line or environment of the session holds the password, and a
//! psql that cannot connect shows its status and waits for Return.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance, stop_and_shutdown};

/// How long a prompt may take to show.
const PROMPT_LIMIT: Duration = Duration::from_secs(20);

/// A script run in a pseudo terminal. Dropping it kills the terminal and
/// with it the psql inside, so no test leaves a session behind.
struct Terminal {
    child: Child,
    output: Arc<Mutex<Vec<u8>>>,
}

impl Terminal {
    /// Runs `script` under `/bin/sh` in a pty with the environment a
    /// Terminal window would give it.
    fn run(script: &Path) -> Self {
        let mut child = Command::new("/usr/bin/script")
            .args(["-q", "/dev/null", "/bin/sh"])
            .arg(script)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("USER", std::env::var("USER").unwrap_or_default())
            .env("TERM", "dumb")
            .env("LANG", "en_US.UTF-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("script starts the pseudo terminal");
        let output = Arc::new(Mutex::new(Vec::new()));
        let mut stdout = child.stdout.take().expect("the output is piped");
        let sink = Arc::clone(&output);
        std::thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            while let Ok(read) = stdout.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                sink.lock()
                    .expect("the output lock")
                    .extend_from_slice(&chunk[..read]);
            }
        });
        Self { child, output }
    }

    /// Everything the terminal showed so far.
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().expect("the output lock")).into_owned()
    }

    /// Types `line` and Return.
    fn type_line(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().expect("the input is piped");
        stdin
            .write_all(format!("{line}\n").as_bytes())
            .expect("the terminal takes the line");
        stdin.flush().expect("the line is sent");
    }

    /// Waits until the screen holds `needle`, and panics with the screen
    /// when it does not within [`PROMPT_LIMIT`].
    fn wait_for(&self, needle: &str) {
        let started = Instant::now();
        while !self.text().contains(needle) {
            assert!(
                started.elapsed() < PROMPT_LIMIT,
                "{needle:?} never showed; the screen holds {:?}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for the terminal to end and returns its exit code.
    fn finish(&mut self) -> Option<i32> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().expect("the terminal can be polled") {
                return status.code();
            }
            assert!(
                started.elapsed() < PROMPT_LIMIT,
                "the terminal never ended; the screen holds {:?}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `ps` with the arguments given, as text.
fn ps(args: &[&str]) -> String {
    let output = Command::new("/bin/ps")
        .args(args)
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

async fn psql_in_a_terminal_asks_for_the_password_runs_a_query_and_leaks_nothing(
    pg: Pg,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "opened").await;
    api.start_ready(made.id).await;
    let version = site
        .psql(made.id, made.port)
        .scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    let script = api
        .prepare_terminal(made.id, "psql")
        .await
        .expect("the psql script is written");
    let mut terminal = Terminal::run(Path::new(&script));
    terminal.wait_for("Password for user postgres:");
    terminal.type_line(&made.password);
    // Typing ahead of the prompt loses the line to the terminal.
    terminal.wait_for("postgres=#");
    // The pty has no window size, so psql would page even one row.
    terminal.type_line("\\pset pager off");
    terminal.wait_for("Pager usage is off.");
    terminal.type_line("SELECT 1;");
    terminal.wait_for("(1 row)");

    // The session is open: no process line or environment holds the
    // password, and the psql line is the runtime's own with its flags. The
    // other versions' tests run at the same time, so a line is ours by port.
    let table = ps(&["-ww", "-axo", "pid,ppid,command"]);
    let psql_lines: Vec<&str> = table
        .lines()
        .filter(|line| {
            line.contains("/bin/psql")
                && line.contains("--username=postgres")
                && line.contains(&format!("--port={}", made.port))
        })
        .collect();
    assert!(!psql_lines.is_empty(), "no psql line in:\n{table}");
    assert!(
        !table.contains(&made.password),
        "the password is in a process line"
    );
    for line in &psql_lines {
        let pid = line
            .split_whitespace()
            .next()
            .expect("a ps line starts with a pid");
        let environment = ps(&["-wwE", "-p", pid, "-o", "command"]);
        assert!(
            !environment.contains("PGPASSWORD") && !environment.contains(&made.password),
            "the environment of psql {pid} holds the password"
        );
    }
    let script_text = std::fs::read_to_string(&script).expect("the script reads");
    assert!(!script_text.contains(&made.password));
    assert!(
        !terminal.text().contains("Press Return"),
        "a connected psql ends silently"
    );

    terminal.type_line("\\q");
    assert_eq!(terminal.finish(), Some(0));
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(
        "the script's psql asked for the password, answered SELECT 1, and no ps line or environment held the password",
    );
}

async fn psql_that_cannot_connect_shows_its_status_and_waits_for_return(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "closed").await;
    api.start_ready(made.id).await;
    let version = site
        .psql(made.id, made.port)
        .scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    let script = api
        .prepare_terminal(made.id, "psql")
        .await
        .expect("the psql script is written");
    // The script outlives the stop, as the spec says; running it now finds
    // no server.
    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;

    let mut terminal = Terminal::run(Path::new(&script));
    terminal.wait_for("psql exited with status 2. Press Return to close.");
    assert!(
        terminal.child.try_wait().expect("polled").is_none(),
        "the window waits for Return"
    );
    terminal.type_line("");
    assert_eq!(terminal.finish(), Some(2));
    daemon.shutdown().await;
    ev.pass_with(
        "a script run against a stopped instance printed 'psql exited with status 2. Press Return to close.', waited, and exited 2 after Return",
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_psql_in_a_terminal_asks_for_the_password_runs_a_query_and_leaks_nothing() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Native,
        expected = "the open-psql.command script of a ready PostgreSQL 18 instance asks for the password in a pty, runs SELECT 1 with the pasted password, and no ps line or environment of the session holds the password",
    );
    psql_in_a_terminal_asks_for_the_password_runs_a_query_and_leaks_nothing(PG18, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_psql_in_a_terminal_asks_for_the_password_runs_a_query_and_leaks_nothing() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Native,
        expected = "the open-psql.command script of a ready PostgreSQL 17 instance asks for the password in a pty, runs SELECT 1 with the pasted password, and no ps line or environment of the session holds the password",
    );
    psql_in_a_terminal_asks_for_the_password_runs_a_query_and_leaks_nothing(PG17, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_psql_that_cannot_connect_shows_its_status_and_waits_for_return() {
    let ev = evidence!(
        covers = ["AC-016", "REQ-016"],
        kind = Kind::Native,
        expected = "the open-psql.command script run after the instance stopped prints the psql exit status 2, waits for Return, and exits 2",
    );
    psql_that_cannot_connect_shows_its_status_and_waits_for_return(PG18, ev).await;
}
