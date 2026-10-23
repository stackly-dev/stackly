//! The dummy server run as a real process, with its own test root and port:
//! every behaviour spec 0009 ("Fake provider dan dummy server") promises
//! the supervisor and walking skeleton tests (AC-15).
#![cfg(unix)]

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use stackly_testkit::{TestRoot, allocate_port};

const BINARY: &str = env!("CARGO_BIN_EXE_stackly-fake-server");
/// Longest wait for a process to start listening or to exit.
const DEADLINE: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(20);
/// How often [`Setup::serve`] tries a fresh port after a bind conflict.
const BIND_ATTEMPTS: usize = 5;
/// Fixture secret in the rendered config; never a real credential.
const FIXTURE_TOKEN: &str = "fixture-auth-token";

/// A dummy server process whose output is collected in the background. It
/// is killed and reaped when dropped, so a failing test leaves no process
/// behind.
struct Fake {
    child: Child,
    stdout: Option<JoinHandle<String>>,
    stderr: Option<JoinHandle<String>>,
    /// Pipes held open but never read, so they fill up.
    unread: (Option<ChildStdout>, Option<ChildStderr>),
}

/// What happens to the read end of a stdout or stderr pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pipe {
    /// Read to the end in the background.
    Collected,
    /// Closed right after the spawn, so every write fails.
    Closed,
    /// Held open and never read, so writes block once the pipe is full.
    Unread,
}

/// How a finished process ended and what it wrote.
#[derive(Debug)]
struct Finished {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

impl Fake {
    /// Starts the binary in `cwd`. With `stdin`, the bytes are written and
    /// stdin is closed; without, stdin is null.
    fn spawn(args: &[String], stdin: Option<&[u8]>, cwd: &Path) -> Self {
        Self::spawn_piped(args, stdin, cwd, Pipe::Collected, Pipe::Collected)
    }

    /// Starts the binary like [`Fake::spawn`], with the given handling of
    /// its stdout and stderr pipes.
    fn spawn_piped(
        args: &[String],
        stdin: Option<&[u8]>,
        cwd: &Path,
        stdout: Pipe,
        stderr: Pipe,
    ) -> Self {
        let mut child = Command::new(BINARY)
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the dummy server binary starts");
        let out = child.stdout.take();
        let err = child.stderr.take();
        let (stdout, held_stdout) = match stdout {
            Pipe::Collected => (out.map(collect), None),
            Pipe::Closed => (None, None),
            Pipe::Unread => (None, out),
        };
        let (stderr, held_stderr) = match stderr {
            Pipe::Collected => (err.map(collect), None),
            Pipe::Closed => (None, None),
            Pipe::Unread => (None, err),
        };
        if let Some(bytes) = stdin {
            let mut pipe = child.stdin.take().expect("stdin was requested as a pipe");
            pipe.write_all(bytes)
                .expect("the dummy server reads stdin before it can fail");
        }
        Self {
            child,
            stdout,
            stderr,
            unread: (held_stdout, held_stderr),
        }
    }

    fn is_running(&mut self) -> bool {
        self.child
            .try_wait()
            .expect("the child status can be polled")
            .is_none()
    }

    fn signal(&self, name: &str) {
        let status = Command::new("kill")
            .args(["-s", name, &self.child.id().to_string()])
            .status()
            .expect("the kill tool runs");
        assert!(status.success(), "kill -s {name} failed with {status}");
    }

    /// Closes the read end of every pipe left unread, so each later write
    /// to it fails. Tests close a pipe this way once the server listens, so
    /// a bind conflict at startup is still seen and retried.
    fn close_unread(&mut self) {
        self.unread = (None, None);
    }

    /// Waits for the process to exit, then returns its status and output.
    fn finish(mut self) -> Finished {
        let started = Instant::now();
        let status = loop {
            if let Some(status) = self
                .child
                .try_wait()
                .expect("the child status can be polled")
            {
                break status;
            }
            assert!(
                started.elapsed() < DEADLINE,
                "the dummy server did not exit within {DEADLINE:?}"
            );
            thread::sleep(POLL);
        };
        // A closed pipe has no output; a pipe left unread is read now that
        // the process is gone, so a startup failure still shows its message.
        let join = |handle: Option<JoinHandle<String>>| {
            handle
                .map(|handle| handle.join().expect("the output reader does not panic"))
                .unwrap_or_default()
        };
        let drain = |pipe: Option<&mut dyn Read>| {
            let mut text = String::new();
            if let Some(pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        };
        let (mut held_stdout, mut held_stderr) = std::mem::take(&mut self.unread);
        let held_stdout = drain(held_stdout.as_mut().map(|pipe| pipe as &mut dyn Read));
        let held_stderr = drain(held_stderr.as_mut().map(|pipe| pipe as &mut dyn Read));
        Finished {
            status,
            stdout: join(self.stdout.take()) + &held_stdout,
            stderr: join(self.stderr.take()) + &held_stderr,
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn collect(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

/// Runs a command that ends on its own.
fn run(root: &TestRoot, args: &[&str], stdin: Option<&[u8]>) -> Finished {
    Fake::spawn(&strings(args), stdin, root.path()).finish()
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// A test root with a data folder, a rendered config, and a free port.
struct Setup {
    root: TestRoot,
    port: u16,
    config: PathBuf,
    data: PathBuf,
}

impl Setup {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let data = root.path().join("data");
        std::fs::create_dir(&data).expect("the test root accepts a data folder");
        let config = root.path().join("fake.json");
        write_config(&config, "hello");
        Self {
            port: allocate_port().expect("a loopback port is free"),
            root,
            config,
            data,
        }
    }

    fn serve_args(&self, extra: &[&str]) -> Vec<String> {
        let mut args = vec![
            "serve".to_owned(),
            format!("--config={}", self.config.display()),
            format!("--data={}", self.data.display()),
            format!("--port={}", self.port),
        ];
        args.extend(strings(extra));
        args
    }

    /// Starts `serve` and waits until it answers `GET /health`. Another
    /// process can take the allocated port before `serve` binds it, so a
    /// bind conflict retries on a fresh port.
    fn serve(&mut self, extra: &[&str]) -> Fake {
        self.serve_piped(extra, Pipe::Collected, Pipe::Collected)
    }

    /// Starts `serve` like [`Setup::serve`], with the given handling of its
    /// stdout and stderr pipes. A bind conflict is only seen and retried
    /// when stderr is collected.
    fn serve_piped(&mut self, extra: &[&str], stdout: Pipe, stderr: Pipe) -> Fake {
        for _ in 0..BIND_ATTEMPTS {
            match self.try_serve(extra, stdout, stderr) {
                Ok(fake) => return fake,
                Err(finished) if finished.stderr.contains("Address already in use") => {
                    self.port = allocate_port().expect("a loopback port is free");
                }
                Err(finished) => panic!("serve exited before it listened: {finished:?}"),
            }
        }
        panic!("serve met a taken port {BIND_ATTEMPTS} times");
    }

    fn try_serve(&self, extra: &[&str], stdout: Pipe, stderr: Pipe) -> Result<Fake, Finished> {
        let mut fake = Fake::spawn_piped(
            &self.serve_args(extra),
            None,
            self.root.path(),
            stdout,
            stderr,
        );
        let started = Instant::now();
        loop {
            if !fake.is_running() {
                return Err(fake.finish());
            }
            if matches!(request(self.port, "GET", "/health"), Ok((200, _))) {
                return Ok(fake);
            }
            assert!(
                started.elapsed() < DEADLINE,
                "serve did not listen within {DEADLINE:?}"
            );
            thread::sleep(POLL);
        }
    }

    fn get(&self, path: &str) -> (u16, String) {
        request(self.port, "GET", path).expect("the dummy server answers")
    }
}

fn write_config(path: &Path, greeting: &str) {
    let config = serde_json::json!({
        "greeting": greeting,
        "instanceName": "dummy",
        "authToken": FIXTURE_TOKEN,
    });
    std::fs::write(path, config.to_string()).expect("the test root accepts the config");
}

/// Sends one raw request and returns the status and body.
fn raw_request(port: u16, raw: &str) -> io::Result<(u16, String)> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(raw.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response
        .split(' ')
        .nth(1)
        .and_then(|status| status.parse().ok())
        .ok_or_else(|| io::Error::other(format!("no status line in {response:?}")))?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    Ok((status, body))
}

fn request(port: u16, method: &str, path: &str) -> io::Result<(u16, String)> {
    raw_request(
        port,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
    )
}

/// Sends `PUT <path>` with `body` and returns the status and body.
fn put(port: u16, path: &str, body: &[u8]) -> io::Result<(u16, String)> {
    let mut raw = format!(
        "PUT {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    raw.extend_from_slice(body);
    // Not through `raw_request`, because a body is not always UTF 8.
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&raw)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let response = String::from_utf8_lossy(&response).into_owned();
    let status = response
        .split(' ')
        .nth(1)
        .and_then(|status| status.parse().ok())
        .ok_or_else(|| io::Error::other(format!("no status line in {response:?}")))?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    Ok((status, body))
}

/// Polls `GET <path>` until `accept` is true for its answer.
fn wait_for(setup: &Setup, path: &str, accept: impl Fn(u16, &str) -> bool) {
    let started = Instant::now();
    loop {
        let (status, body) = setup.get(path);
        if accept(status, &body) {
            return;
        }
        assert!(
            started.elapsed() < DEADLINE,
            "GET {path} still answers {status} {body:?}"
        );
        thread::sleep(POLL);
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn marker_line() -> String {
    format!(
        "{}.{}\n",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR")
    )
}

fn read_argv(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).expect("the argv record exists");
    serde_json::from_slice(&bytes).expect("the argv record is a json array of strings")
}

#[test]
fn version_prints_the_crate_name_and_version() {
    let root = TestRoot::new().unwrap();

    let finished = run(&root, &["version"], None);

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    assert_eq!(
        finished.stdout,
        format!("stackly-fake-server {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn bad_arguments_exit_with_two() {
    let setup = Setup::new();
    let data = format!("--data={}", setup.data.display());
    let config = format!("--config={}", setup.config.display());
    let cases: Vec<Vec<String>> = vec![
        strings(&[]),
        strings(&["bogus"]),
        strings(&["version", "--verbose"]),
        strings(&["init"]),
        strings(&["init", "--data"]),
        strings(&["init", &data, "--token-stdin=yes"]),
        strings(&["init", &data, "--fail=maybe"]),
        strings(&["init", &data, "--shell=true"]),
        strings(&["serve", &config, &data]),
        setup.serve_args(&["--port=1"]),
        setup.serve_args(&["--crash-exit-code=0"]),
        setup.serve_args(&["--crash-exit-code=256"]),
        setup.serve_args(&["--ready-delay-ms=-1"]),
        setup.serve_args(&["--log-lines-per-second=fast"]),
        setup.serve_args(&["--ignore-term=yes"]),
        setup.serve_args(&["--host=0.0.0.0"]),
        setup.serve_args(&["--token-stdin"]),
        strings(&["shutdown"]),
        strings(&["shutdown", "--port=0"]),
        strings(&["shutdown", "--port=80", "--host=10.0.0.1"]),
    ];

    for args in cases {
        let finished = Fake::spawn(&args, None, setup.root.path()).finish();

        assert_eq!(finished.status.code(), Some(2), "{args:?}: {finished:?}");
        assert!(
            finished.stderr.starts_with("stackly-fake-server: "),
            "{args:?}: {finished:?}"
        );
    }
    assert_eq!(std::fs::read_dir(&setup.data).unwrap().count(), 0);
}

#[test]
fn init_records_argv_marker_and_the_digest_of_the_exact_stdin_bytes() {
    let root = TestRoot::new().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let token = format!("{FIXTURE_TOKEN} \u{fc}\n");
    let args = vec![
        "init".to_owned(),
        format!("--data={}", data.display()),
        "--token-stdin".to_owned(),
    ];

    let finished = Fake::spawn(&args, Some(token.as_bytes()), root.path()).finish();

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    assert_eq!(read_argv(&data.join("argv-init.json")), args);
    let digest = std::fs::read_to_string(data.join("token.sha256")).unwrap();
    assert_eq!(digest, sha256_hex(token.as_bytes()));
    assert_ne!(digest, sha256_hex(token.trim_end().as_bytes()));
    assert_eq!(
        std::fs::read_to_string(data.join("FAKE_VERSION")).unwrap(),
        marker_line()
    );
    for entry in std::fs::read_dir(&data).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(FIXTURE_TOKEN),
            "the token itself is never written"
        );
    }
    assert!(!finished.stdout.contains(FIXTURE_TOKEN));
    assert!(!finished.stderr.contains(FIXTURE_TOKEN));
}

#[test]
fn init_without_token_stdin_writes_no_digest() {
    let root = TestRoot::new().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();

    let finished = run(
        &root,
        &["init", &format!("--data={}", data.display())],
        None,
    );

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    assert!(!data.join("token.sha256").exists());
    assert_eq!(
        std::fs::read_to_string(data.join("FAKE_VERSION")).unwrap(),
        marker_line()
    );
}

#[test]
fn init_creates_a_missing_data_folder_as_private() {
    let root = TestRoot::new().unwrap();
    let data = root.path().join("instances/one/data");

    let finished = run(
        &root,
        &["init", &format!("--data={}", data.display())],
        None,
    );

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    let mode = std::fs::metadata(&data).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    assert!(data.join("FAKE_VERSION").is_file());
}

#[test]
fn init_refuses_a_data_folder_that_holds_anything_else() {
    let root = TestRoot::new().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    std::fs::write(data.join("leftover"), b"from an earlier run").unwrap();

    let finished = run(
        &root,
        &["init", &format!("--data={}", data.display())],
        None,
    );

    assert_eq!(finished.status.code(), Some(3), "{finished:?}");
    assert!(finished.stderr.contains("not empty"), "{finished:?}");
    assert!(finished.stderr.contains("leftover"), "{finished:?}");
    assert!(!data.join("FAKE_VERSION").exists());
}

#[test]
fn init_accepts_a_data_folder_that_holds_only_an_earlier_argv_record() {
    let root = TestRoot::new().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let data_flag = format!("--data={}", data.display());

    let failed = run(&root, &["init", &data_flag, "--fail=true"], None);
    let retried = run(&root, &["init", &data_flag], None);
    let again = run(&root, &["init", &data_flag], None);

    assert_eq!(failed.status.code(), Some(4), "{failed:?}");
    assert_eq!(retried.status.code(), Some(0), "{retried:?}");
    assert_eq!(again.status.code(), Some(3), "{again:?}");
}

#[test]
fn init_with_fail_true_exits_with_four_and_leaves_only_the_argv_record() {
    let root = TestRoot::new().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let args = vec![
        "init".to_owned(),
        format!("--data={}", data.display()),
        "--token-stdin".to_owned(),
        "--fail=true".to_owned(),
    ];

    let finished = Fake::spawn(&args, Some(FIXTURE_TOKEN.as_bytes()), root.path()).finish();

    assert_eq!(finished.status.code(), Some(4), "{finished:?}");
    let entries: Vec<_> = std::fs::read_dir(&data)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, ["argv-init.json"]);
    assert_eq!(read_argv(&data.join("argv-init.json")), args);
}

/// Spec 0014 AC-4.
#[test]
fn init_delay_waits_after_the_token_and_before_touching_the_data_folder() {
    let root = TestRoot::new().unwrap();
    let data = root.path().join("instances/one/data");
    let args = vec![
        "init".to_owned(),
        format!("--data={}", data.display()),
        "--token-stdin".to_owned(),
        "--delay-ms=1200".to_owned(),
    ];
    let started = Instant::now();

    let fake = Fake::spawn(&args, Some(FIXTURE_TOKEN.as_bytes()), root.path());
    thread::sleep(Duration::from_millis(400));
    assert!(
        !data.exists(),
        "the data folder stays untouched during the delay"
    );
    let finished = fake.finish();

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    assert!(started.elapsed() >= Duration::from_millis(1200));
    assert_eq!(
        std::fs::read_to_string(data.join("token.sha256")).unwrap(),
        sha256_hex(FIXTURE_TOKEN.as_bytes()),
        "the token is still read in full"
    );
    assert_eq!(read_argv(&data.join("argv-init.json")), args);
}

/// Spec 0014 AC-4.
#[test]
fn init_with_a_delay_that_is_not_a_number_is_a_usage_failure() {
    let root = TestRoot::new().unwrap();
    let data = root.path().join("data");

    let finished = run(
        &root,
        &[
            "init",
            &format!("--data={}", data.display()),
            "--delay-ms=soon",
        ],
        None,
    );

    assert_eq!(finished.status.code(), Some(2), "{finished:?}");
    assert!(!data.exists());
}

#[test]
fn serve_answers_ready_only_after_the_delay() {
    let mut setup = Setup::new();
    let started = Instant::now();

    let fake = setup.serve(&["--ready-delay-ms=1500"]);

    assert_eq!(setup.get("/ready"), (503, "not ready\n".to_owned()));
    wait_for(&setup, "/ready", |status, _| status == 200);
    assert!(
        started.elapsed() >= Duration::from_millis(1500),
        "ready after {:?}",
        started.elapsed()
    );
    assert_eq!(setup.get("/health"), (200, "ok\n".to_owned()));
    drop(fake);
}

#[test]
fn serve_with_no_delay_answers_every_route() {
    let mut setup = Setup::new();
    let _fake = setup.serve(&[]);

    assert_eq!(setup.get("/ready"), (200, "ready\n".to_owned()));
    assert_eq!(setup.get("/ready?attempt=2"), (200, "ready\n".to_owned()));
    assert_eq!(setup.get("/health"), (200, "ok\n".to_owned()));
    let (status, body) = setup.get("/config");
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({ "greeting": "hello" })
    );
    assert!(!body.contains(FIXTURE_TOKEN));
    assert_eq!(setup.get("/missing").0, 404);
    assert_eq!(setup.get("/shutdown").0, 404);
    assert_eq!(request(setup.port, "POST", "/health").unwrap().0, 404);
    assert_eq!(raw_request(setup.port, "NONSENSE\r\n\r\n").unwrap().0, 400);
    assert_eq!(setup.get("/health").0, 200);
}

// covers: AC-7, AC-8 (spec 0024, the sentinel route is the tool of the proofs)
#[test]
fn sentinel_is_written_and_read_back_from_the_data_folder() {
    let mut setup = Setup::new();
    let _fake = setup.serve(&[]);

    assert_eq!(setup.get("/sentinel"), (404, "not found\n".to_owned()));
    assert_eq!(
        put(setup.port, "/sentinel", b"alpha").unwrap(),
        (204, String::new())
    );
    assert_eq!(setup.get("/sentinel"), (200, "alpha".to_owned()));
    assert_eq!(
        std::fs::read(setup.data.join("SENTINEL")).unwrap(),
        b"alpha"
    );

    assert_eq!(
        put(setup.port, "/sentinel", "βeta ✓".as_bytes()).unwrap().0,
        204
    );
    assert_eq!(setup.get("/sentinel"), (200, "βeta ✓".to_owned()));
    let mut names: Vec<_> = std::fs::read_dir(&setup.data)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["SENTINEL", "argv-serve.json"],
        "no temporary file is left behind"
    );
}

// covers: AC-8 (spec 0024, the sentinel route is the tool of the proofs)
#[test]
fn sentinel_accepts_one_to_two_hundred_fifty_six_bytes() {
    let mut setup = Setup::new();
    let _fake = setup.serve(&[]);

    assert_eq!(put(setup.port, "/sentinel", b"x").unwrap().0, 204);
    let longest = "y".repeat(256);
    assert_eq!(
        put(setup.port, "/sentinel", longest.as_bytes()).unwrap().0,
        204
    );
    assert_eq!(setup.get("/sentinel"), (200, longest.clone()));

    for body in [&b""[..], "z".repeat(257).as_bytes(), &[0xff, 0xfe, 0x41]] {
        assert_eq!(
            put(setup.port, "/sentinel", body).unwrap().0,
            400,
            "{body:?}"
        );
    }
    assert_eq!(
        setup.get("/sentinel"),
        (200, longest),
        "a refused body leaves the stored sentinel alone"
    );
}

// covers: AC-8 (spec 0024, the sentinel route is the tool of the proofs)
#[test]
fn sentinel_routes_answer_only_their_methods() {
    let mut setup = Setup::new();
    let _fake = setup.serve(&[]);

    assert_eq!(request(setup.port, "POST", "/sentinel").unwrap().0, 404);
    assert_eq!(request(setup.port, "DELETE", "/sentinel").unwrap().0, 404);
    assert_eq!(put(setup.port, "/other", b"x").unwrap().0, 404);
    assert_eq!(setup.get("/sentinel").0, 404);
}

// covers: AC-7, AC-8 (spec 0024, the sentinel route is the tool of the proofs)
#[test]
fn two_servers_keep_their_own_sentinels() {
    let mut first = Setup::new();
    let mut second = Setup::new();
    let _one = first.serve(&[]);
    let _two = second.serve(&[]);

    assert_eq!(put(first.port, "/sentinel", b"one").unwrap().0, 204);
    assert_eq!(second.get("/sentinel").0, 404);
    assert_eq!(put(second.port, "/sentinel", b"two").unwrap().0, 204);
    assert_eq!(first.get("/sentinel"), (200, "one".to_owned()));
    assert_eq!(second.get("/sentinel"), (200, "two".to_owned()));
}

// covers: AC-7, AC-8 (spec 0024, the sentinel route is the tool of the proofs)
#[test]
fn sentinel_survives_a_restart_on_the_same_data_folder() {
    let mut setup = Setup::new();
    let fake = setup.serve(&[]);
    assert_eq!(put(setup.port, "/sentinel", b"kept").unwrap().0, 204);
    drop(fake);

    let _again = setup.serve(&[]);

    assert_eq!(setup.get("/sentinel"), (200, "kept".to_owned()));
}

#[test]
fn serve_records_argv_exactly_as_received_without_a_shell() {
    let mut setup = Setup::new();
    let label = r#"--label=$(touch pwned); rm -rf ~ `id` "q" 'q' | & > ü 空"#;

    let fake = setup.serve(&[label]);

    let args = setup.serve_args(&[label]);
    assert_eq!(read_argv(&setup.data.join("argv-serve.json")), args);
    assert!(!setup.root.path().join("pwned").exists());
    drop(fake);
}

#[test]
fn serve_crashes_with_the_given_exit_code_after_the_delay() {
    let mut setup = Setup::new();
    let started = Instant::now();

    let fake = setup.serve(&["--crash-after-ms=400", "--crash-exit-code=42"]);
    let finished = fake.finish();

    assert_eq!(finished.status.code(), Some(42), "{finished:?}");
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert!(
        finished
            .stderr
            .contains("crashing on purpose with exit code 42"),
        "{finished:?}"
    );
}

#[test]
fn serve_crash_exit_code_defaults_to_seventy() {
    let mut setup = Setup::new();

    let finished = setup.serve(&["--crash-after-ms=200"]).finish();

    assert_eq!(finished.status.code(), Some(70), "{finished:?}");
}

#[test]
fn serve_spams_hundred_byte_log_lines_on_stdout_and_stderr_in_turn() {
    let mut setup = Setup::new();
    let fake = setup.serve(&["--log-lines-per-second=200"]);
    let started = Instant::now();

    thread::sleep(Duration::from_millis(1000));
    fake.signal("TERM");
    let finished = fake.finish();
    let elapsed = started.elapsed();

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    let spam = |text: &str, stream: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with("log line "))
            .inspect(|line| {
                assert_eq!(line.len() + 1, 100, "{line:?}");
                assert!(line.contains(&format!(" on {stream} ")), "{line:?}");
            })
            .map(str::to_owned)
            .collect()
    };
    let out = spam(&finished.stdout, "stdout");
    let err = spam(&finished.stderr, "stderr");
    assert!(out.len() >= 50, "{} stdout lines", out.len());
    assert!(err.len() >= 50, "{} stderr lines", err.len());
    let budget = elapsed.as_millis() * 200 / 1000 + 10;
    let total = out.len() + err.len();
    assert!(total as u128 <= budget, "{total} lines in {elapsed:?}");
    assert!(out[0].starts_with("log line 0000000000 on stdout"));
    assert!(err[0].starts_with("log line 0000000001 on stderr"));
}

#[test]
fn serve_writes_no_log_spam_by_default() {
    let mut setup = Setup::new();
    let fake = setup.serve(&[]);

    thread::sleep(Duration::from_millis(300));
    fake.signal("TERM");
    let finished = fake.finish();

    assert!(!finished.stdout.contains("log line "), "{finished:?}");
    assert!(!finished.stderr.contains("log line "), "{finished:?}");
}

#[test]
fn sigterm_and_sigint_exit_zero_cleanly() {
    for signal in ["TERM", "INT"] {
        let mut setup = Setup::new();
        let fake = setup.serve(&[]);

        fake.signal(signal);
        let finished = fake.finish();

        assert_eq!(finished.status.code(), Some(0), "{signal}: {finished:?}");
        assert!(
            finished
                .stdout
                .contains(&format!("received SIG{signal}, exiting")),
            "{signal}: {finished:?}"
        );
    }
}

#[test]
fn sigterm_and_post_shutdown_exit_zero_when_stdout_is_closed() {
    for stop in ["SIGTERM", "POST /shutdown"] {
        let mut setup = Setup::new();
        let fake = setup.serve_piped(&[], Pipe::Closed, Pipe::Collected);

        if stop == "SIGTERM" {
            fake.signal("TERM");
        } else {
            let _ = request(setup.port, "POST", "/shutdown");
        }
        let finished = fake.finish();

        assert_eq!(finished.status.code(), Some(0), "{stop}: {finished:?}");
    }
}

#[test]
fn sigterm_exits_zero_while_nothing_drains_a_full_stdout_and_stderr() {
    let mut setup = Setup::new();
    let fake = setup.serve_piped(&["--log-lines-per-second=5000"], Pipe::Unread, Pipe::Unread);
    // 2500 lines of 100 bytes a second on each stream fill a 64 KiB pipe
    // well within this pause.
    thread::sleep(Duration::from_millis(500));

    fake.signal("TERM");
    let finished = fake.finish();

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
}

#[test]
fn a_crash_exits_with_its_code_when_stderr_is_closed() {
    let mut setup = Setup::new();

    let mut fake = setup.serve_piped(
        &["--crash-after-ms=1500", "--crash-exit-code=42"],
        Pipe::Collected,
        Pipe::Unread,
    );

    fake.close_unread();
    let finished = fake.finish();

    assert_eq!(finished.status.code(), Some(42), "{finished:?}");
}

#[test]
fn notes_on_closed_streams_never_stop_the_signal_watcher() {
    let mut setup = Setup::new();
    let mut fake = setup.serve_piped(&["--ignore-term=true"], Pipe::Unread, Pipe::Unread);
    fake.close_unread();

    fake.signal("HUP");
    fake.signal("TERM");
    thread::sleep(Duration::from_millis(300));
    assert!(fake.is_running(), "an ignored SIGTERM keeps the server up");
    fake.signal("INT");
    let finished = fake.finish();

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
}

#[test]
fn ignore_term_ignores_sigterm_but_still_exits_on_sigint() {
    let mut setup = Setup::new();
    let mut fake = setup.serve(&["--ignore-term=true"]);

    fake.signal("TERM");
    thread::sleep(Duration::from_millis(500));
    assert!(fake.is_running(), "SIGTERM must be ignored");
    assert_eq!(setup.get("/health").0, 200);
    fake.signal("INT");
    let finished = fake.finish();

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    assert!(finished.stdout.contains("ignoring SIGTERM"), "{finished:?}");
    assert!(
        finished.stdout.contains("received SIGINT, exiting"),
        "{finished:?}"
    );
}

#[test]
fn sighup_reloads_the_config() {
    let mut setup = Setup::new();
    let fake = setup.serve(&[]);
    assert_eq!(setup.get("/config").1, r#"{"greeting":"hello"}"#);

    write_config(&setup.config, "bonjour");
    fake.signal("HUP");
    wait_for(&setup, "/config", |_, body| {
        body == r#"{"greeting":"bonjour"}"#
    });
    fake.signal("TERM");
    let finished = fake.finish();

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    assert!(finished.stdout.contains("reloaded config"), "{finished:?}");
}

#[test]
fn sighup_with_a_broken_config_keeps_the_previous_one() {
    let mut setup = Setup::new();
    let fake = setup.serve(&[]);

    let broken = format!(r#"{{"greeting": "hola", "authToken": "{FIXTURE_TOKEN}"}}"#);
    std::fs::write(&setup.config, broken).unwrap();
    fake.signal("HUP");
    thread::sleep(Duration::from_millis(300));
    assert_eq!(setup.get("/config").1, r#"{"greeting":"hello"}"#);
    fake.signal("TERM");
    let finished = fake.finish();

    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
    assert!(
        finished
            .stderr
            .contains("cannot reload config, keeping the previous one"),
        "{finished:?}"
    );
    assert!(!finished.stderr.contains(FIXTURE_TOKEN));
    assert!(!finished.stdout.contains("reloaded config"));
}

#[test]
fn post_shutdown_answers_200_and_exits_zero() {
    let mut setup = Setup::new();
    let fake = setup.serve(&[]);

    let answer = request(setup.port, "POST", "/shutdown").unwrap();
    let finished = fake.finish();

    assert_eq!(answer, (200, "shutting down\n".to_owned()));
    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
}

#[test]
fn shutdown_command_stops_a_server_that_ignores_sigterm() {
    let mut setup = Setup::new();
    let fake = setup.serve(&["--ignore-term=true"]);

    let shutdown = run(
        &setup.root,
        &["shutdown", &format!("--port={}", setup.port)],
        None,
    );
    let finished = fake.finish();

    assert_eq!(shutdown.status.code(), Some(0), "{shutdown:?}");
    assert_eq!(finished.status.code(), Some(0), "{finished:?}");
}

#[test]
fn shutdown_exits_with_one_when_nothing_listens() {
    let root = TestRoot::new().unwrap();
    let port = allocate_port().unwrap();

    let finished = run(&root, &["shutdown", &format!("--port={port}")], None);

    assert_eq!(finished.status.code(), Some(1), "{finished:?}");
}

#[test]
fn shutdown_exits_with_one_when_the_server_answers_otherwise() {
    let root = TestRoot::new().unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = [0_u8; 1024];
        let _ = stream.read(&mut head).unwrap();
        stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
    });

    let finished = run(&root, &["shutdown", &format!("--port={port}")], None);
    server.join().unwrap();

    assert_eq!(finished.status.code(), Some(1), "{finished:?}");
    assert!(finished.stderr.contains("404"), "{finished:?}");
}

#[test]
fn serve_exits_with_one_on_a_config_it_cannot_use() {
    let setup = Setup::new();
    let cases = [
        "not json".to_owned(),
        "[]".to_owned(),
        r#"{"greeting": "hi", "instanceName": "x"}"#.to_owned(),
        format!(r#"{{"greeting": 1, "instanceName": "x", "authToken": "{FIXTURE_TOKEN}"}}"#),
        format!(
            r#"{{"greeting": "hi", "instanceName": "x", "authToken": "{FIXTURE_TOKEN}", "shell": "sh"}}"#
        ),
    ];

    for config in cases {
        std::fs::write(&setup.config, &config).unwrap();

        let finished = Fake::spawn(&setup.serve_args(&[]), None, setup.root.path()).finish();

        assert_eq!(finished.status.code(), Some(1), "{config}: {finished:?}");
        assert!(finished.stderr.contains("config"), "{finished:?}");
        assert!(!finished.stderr.contains(FIXTURE_TOKEN), "{finished:?}");
    }
}

#[test]
fn serve_exits_with_one_when_the_port_is_taken() {
    let mut setup = Setup::new();
    // Another process can take the allocated port first; then take a fresh one.
    let _taken = (0..BIND_ATTEMPTS)
        .find_map(
            |_| match TcpListener::bind((Ipv4Addr::LOCALHOST, setup.port)) {
                Ok(listener) => Some(listener),
                Err(_) => {
                    setup.port = allocate_port().expect("a loopback port is free");
                    None
                }
            },
        )
        .expect("a loopback port can be held for the test");

    let finished = Fake::spawn(&setup.serve_args(&[]), None, setup.root.path()).finish();

    assert_eq!(finished.status.code(), Some(1), "{finished:?}");
    assert!(finished.stderr.contains("cannot listen"), "{finished:?}");
}
