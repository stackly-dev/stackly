//! `serve`: the long running process a supervisor starts, probes, reloads,
//! and stops.

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::args::Flags;
use crate::http::{self, IO_TIMEOUT, RequestError};
use crate::{Failure, write_argv};

/// The argv record `serve` writes in the data folder.
const ARGV_FILE: &str = "argv-serve.json";
/// Every key the rendered `fakeConfig` holds, and no other.
const CONFIG_KEYS: [&str; 3] = ["greeting", "instanceName", "authToken"];
/// The file `PUT /sentinel` writes in the data folder.
const SENTINEL_FILE: &str = "SENTINEL";
/// Largest sentinel `PUT /sentinel` takes, in bytes.
const MAX_SENTINEL_BYTES: usize = 256;
/// Largest config file `serve` reads.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// Exit code of a crash when `--crash-exit-code` is absent, the same as
/// the `crashExitCode` default in the `testing/fake` settings.
const DEFAULT_CRASH_EXIT_CODE: u8 = 70;
/// Every spam line is this long, newline included.
const LOG_LINE_BYTES: usize = 100;
/// Longest sleep between two checks of the log line schedule.
const LOG_TICK: Duration = Duration::from_millis(50);
/// Longest wait for the last line before the process exits anyway.
const FAREWELL_WAIT: Duration = Duration::from_millis(200);
const TEXT: &str = "text/plain; charset=utf-8";
const JSON: &str = "application/json";

/// Runs `serve` until a signal, a crash, or `POST /shutdown` ends the
/// process. Returns only when startup fails.
pub(crate) fn run(argv: &[String], args: &[String]) -> Result<(), Failure> {
    let flags = Flags::parse(
        args,
        &[
            "config",
            "data",
            "port",
            "host",
            "label",
            "ready-delay-ms",
            "crash-after-ms",
            "crash-exit-code",
            "log-lines-per-second",
            "ignore-term",
        ],
        &[],
    )?;
    let options = Options::from_flags(&flags)?;

    // Install the handlers before anything else, so a signal that arrives
    // during startup is handled instead of killing the process.
    let signals = Signals::new([SIGTERM, SIGINT, SIGHUP])
        .map_err(|error| Failure::Runtime(format!("cannot install signal handlers: {error}")))?;
    write_argv(&options.data.join(ARGV_FILE), argv)?;
    let config = Config::read(&options.config).map_err(Failure::Runtime)?;
    let address = SocketAddr::new(options.host, options.port);
    let listener = TcpListener::bind(address)
        .map_err(|error| Failure::Runtime(format!("cannot listen on {address}: {error}")))?;

    let server = Arc::new(Server {
        config_path: options.config,
        data: options.data,
        config: Mutex::new(config),
        started: Instant::now(),
        ready_delay: options.ready_delay,
    });
    // Nothing reads stdout yet only when the caller closed it; serving goes on.
    let _ = writeln!(io::stdout(), "listening on {address}");

    let watcher = Arc::clone(&server);
    let ignore_term = options.ignore_term;
    thread::spawn(move || watch_signals(signals, &watcher, ignore_term));
    if let Some(after) = options.crash_after {
        let code = options.crash_exit_code;
        thread::spawn(move || crash(after, code));
    }
    if options.log_lines_per_second > 0 {
        let rate = options.log_lines_per_second;
        thread::spawn(move || spam_logs(rate));
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let server = Arc::clone(&server);
                thread::spawn(move || server.handle(stream));
            }
            Err(error) => {
                note(Stream::Stderr, format!("accept failed: {error}"));
            }
        }
    }
    Ok(())
}

/// The checked `serve` flags. `--label` is accepted only so it lands in the
/// argv record.
#[derive(Debug)]
struct Options {
    config: PathBuf,
    data: PathBuf,
    host: IpAddr,
    port: u16,
    ready_delay: Duration,
    /// `None` when `--crash-after-ms` is absent or 0.
    crash_after: Option<Duration>,
    crash_exit_code: u8,
    log_lines_per_second: u32,
    ignore_term: bool,
}

impl Options {
    fn from_flags(flags: &Flags) -> Result<Self, Failure> {
        let crash_after_ms = flags.number("crash-after-ms", 0_u64)?;
        let crash_exit_code = flags.number("crash-exit-code", DEFAULT_CRASH_EXIT_CODE)?;
        if crash_exit_code == 0 {
            return Err(Failure::Usage(
                "--crash-exit-code must be 1 to 255, got \"0\"".to_owned(),
            ));
        }
        Ok(Self {
            config: flags.required_path("config")?,
            data: flags.required_path("data")?,
            host: flags.host()?,
            port: flags.port()?,
            ready_delay: Duration::from_millis(flags.number("ready-delay-ms", 0)?),
            crash_after: (crash_after_ms > 0).then(|| Duration::from_millis(crash_after_ms)),
            crash_exit_code,
            log_lines_per_second: flags.number("log-lines-per-second", 0)?,
            ignore_term: flags.boolean("ignore-term", false)?,
        })
    }
}

/// The rendered `fakeConfig`. Only `greeting` is served back; the other
/// keys are checked so a template that drifts fails at once.
#[derive(Debug)]
struct Config {
    greeting: String,
}

impl Config {
    /// Reads and checks the config. The message never holds a config value,
    /// because `authToken` is a secret.
    fn read(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|error| format!("cannot read config {}: {error}", path.display()))?;
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            return Err(format!("config {} is larger than 1 MiB", path.display()));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("config {} is not valid json: {error}", path.display()))?;
        let object = value
            .as_object()
            .ok_or_else(|| format!("config {} is not a json object", path.display()))?;
        if let Some(key) = object
            .keys()
            .find(|key| !CONFIG_KEYS.contains(&key.as_str()))
        {
            return Err(format!(
                "config {} has an unknown key {key:?}",
                path.display()
            ));
        }
        let text = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("config {} needs {key} as a string", path.display()))
        };
        let greeting = text("greeting")?.to_owned();
        text("instanceName")?;
        text("authToken")?;
        Ok(Self { greeting })
    }
}

/// State every connection and the signal watcher share.
#[derive(Debug)]
struct Server {
    config_path: PathBuf,
    data: PathBuf,
    config: Mutex<Config>,
    started: Instant,
    ready_delay: Duration,
}

impl Server {
    fn handle(&self, mut stream: TcpStream) {
        // A peer that cannot take the timeouts is dropped like any other
        // broken connection.
        if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
            || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
        {
            return;
        }
        let request = match http::read_request(&mut stream) {
            Ok(request) => request,
            Err(RequestError::Malformed) => {
                let _ = http::write_response(&mut stream, 400, TEXT, b"bad request\n");
                return;
            }
            Err(RequestError::Closed) => return,
        };
        let _ = match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/ready") if self.started.elapsed() >= self.ready_delay => {
                http::write_response(&mut stream, 200, TEXT, b"ready\n")
            }
            ("GET", "/ready") => http::write_response(&mut stream, 503, TEXT, b"not ready\n"),
            ("GET", "/health") => http::write_response(&mut stream, 200, TEXT, b"ok\n"),
            ("GET", "/config") => {
                let body = serde_json::json!({ "greeting": self.greeting() }).to_string();
                http::write_response(&mut stream, 200, JSON, body.as_bytes())
            }
            ("PUT", "/sentinel") => self.put_sentinel(&mut stream, &request.body),
            ("GET", "/sentinel") => self.get_sentinel(&mut stream),
            ("POST", "/shutdown") => {
                let _ = http::write_response(&mut stream, 200, TEXT, b"shutting down\n");
                let _ = stream.shutdown(Shutdown::Write);
                exit_with(0, Stream::Stdout, "shutdown requested, exiting".to_owned());
            }
            _ => http::write_response(&mut stream, 404, TEXT, b"not found\n"),
        };
    }

    /// Stores `body` as the sentinel, or answers 400 when it is not 1 to
    /// [`MAX_SENTINEL_BYTES`] bytes of UTF 8.
    fn put_sentinel(&self, stream: &mut TcpStream, body: &[u8]) -> io::Result<()> {
        if body.is_empty() || body.len() > MAX_SENTINEL_BYTES || std::str::from_utf8(body).is_err()
        {
            return http::write_response(
                stream,
                400,
                TEXT,
                b"the sentinel is 1 to 256 bytes of utf 8\n",
            );
        }
        match write_sentinel(&self.data, body) {
            Ok(()) => http::write_response(stream, 204, TEXT, b""),
            Err(error) => {
                note(
                    Stream::Stderr,
                    format!("cannot write the sentinel: {error}"),
                );
                http::write_response(stream, 500, TEXT, b"cannot write the sentinel\n")
            }
        }
    }

    /// Answers the sentinel of this process's own data folder, or 404.
    fn get_sentinel(&self, stream: &mut TcpStream) -> io::Result<()> {
        match std::fs::read(self.data.join(SENTINEL_FILE)) {
            Ok(bytes) => http::write_response(stream, 200, TEXT, &bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                http::write_response(stream, 404, TEXT, b"not found\n")
            }
            Err(error) => {
                note(Stream::Stderr, format!("cannot read the sentinel: {error}"));
                http::write_response(stream, 500, TEXT, b"cannot read the sentinel\n")
            }
        }
    }

    fn greeting(&self) -> String {
        self.config
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .greeting
            .clone()
    }

    /// Reads the config again. A config that no longer reads keeps the
    /// previous one in place, like a real server that rejects a bad reload.
    fn reload(&self) {
        match Config::read(&self.config_path) {
            Ok(config) => {
                *self.config.lock().unwrap_or_else(PoisonError::into_inner) = config;
                note(Stream::Stdout, "reloaded config".to_owned());
            }
            Err(message) => {
                note(
                    Stream::Stderr,
                    format!("cannot reload config, keeping the previous one: {message}"),
                );
            }
        }
    }
}

/// Writes the sentinel through a temporary file in the same folder and a
/// rename, so a reader sees the old value or the new one, never a part.
fn write_sentinel(data: &Path, body: &[u8]) -> io::Result<()> {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let temporary = data.join(format!(
        "{SENTINEL_FILE}.{}.tmp",
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    let result = std::fs::write(&temporary, body)
        .and_then(|()| std::fs::rename(&temporary, data.join(SENTINEL_FILE)));
    if result.is_err() {
        // The write error is the one worth reporting.
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// SIGTERM and SIGINT end the process with exit 0, except that
/// `--ignore-term=true` ignores SIGTERM. SIGHUP reloads the config.
fn watch_signals(mut signals: Signals, server: &Server, ignore_term: bool) {
    for signal in signals.forever() {
        match signal {
            SIGHUP => server.reload(),
            SIGTERM if ignore_term => {
                note(Stream::Stdout, "ignoring SIGTERM".to_owned());
            }
            SIGTERM => exit_on("SIGTERM"),
            SIGINT => exit_on("SIGINT"),
            _ => {}
        }
    }
}

fn exit_on(signal: &str) -> ! {
    exit_with(0, Stream::Stdout, format!("received {signal}, exiting"));
}

fn crash(after: Duration, code: u8) -> ! {
    thread::sleep(after);
    exit_with(
        i32::from(code),
        Stream::Stderr,
        format!("crashing on purpose with exit code {code}"),
    );
}

/// The stream a note goes to.
#[derive(Debug, Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

/// Writes one line to `stream` from a thread of its own and returns at
/// once, so a stream that is closed, full, or held by the spam thread never
/// stops the caller, such as the signal watcher. The receiver gets a
/// message, or sees its sender dropped, once the write is done or failed.
fn note(stream: Stream, line: String) -> mpsc::Receiver<()> {
    let (done, finished) = mpsc::channel();
    // Without a thread the line is simply not written.
    let _ = thread::Builder::new().spawn(move || {
        let _ = match stream {
            Stream::Stdout => writeln!(io::stdout(), "{line}"),
            Stream::Stderr => writeln!(io::stderr(), "{line}"),
        };
        let _ = done.send(());
    });
    finished
}

/// Writes `line` as a farewell and exits with `code`, waiting at most
/// [`FAREWELL_WAIT`] for the write: the exit never depends on stdout or
/// stderr.
fn exit_with(code: i32, stream: Stream, line: String) -> ! {
    let _ = note(stream, line).recv_timeout(FAREWELL_WAIT);
    std::process::exit(code);
}

/// Writes `rate` lines a second, alternating stdout and stderr, until a
/// stream is closed. Lines due at once are written in a burst, so high rates
/// keep pace without a sleep per line.
fn spam_logs(rate: u32) {
    let started = Instant::now();
    let rate = u128::from(rate);
    let mut written: u64 = 0;
    loop {
        let due = started.elapsed().as_micros() * rate / 1_000_000 + 1;
        while u128::from(written) < due {
            let result = if written.is_multiple_of(2) {
                io::stdout()
                    .lock()
                    .write_all(log_line(written, "stdout").as_bytes())
            } else {
                io::stderr()
                    .lock()
                    .write_all(log_line(written, "stderr").as_bytes())
            };
            if result.is_err() {
                return;
            }
            written += 1;
        }
        let next_micros = u128::from(written) * 1_000_000 / rate;
        let next = Duration::from_micros(u64::try_from(next_micros).unwrap_or(u64::MAX));
        thread::sleep(next.saturating_sub(started.elapsed()).min(LOG_TICK));
    }
}

/// One spam line of exactly [`LOG_LINE_BYTES`], newline included.
fn log_line(sequence: u64, stream: &str) -> String {
    let text = format!("log line {sequence:010} on {stream} ");
    format!("{text:.<width$}\n", width = LOG_LINE_BYTES - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_are_exactly_one_hundred_bytes() {
        let line = log_line(42, "stderr");

        assert_eq!(line.len(), LOG_LINE_BYTES);
        assert!(line.starts_with("log line 0000000042 on stderr ."));
        assert!(line.ends_with(".\n"));
        assert_eq!(log_line(u64::MAX, "stdout").len(), LOG_LINE_BYTES);
    }
}
