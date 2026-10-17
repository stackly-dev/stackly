//! `cargo xtask runtime spike-worker --plan <file>`: the process launchd
//! starts (spec 0003). It runs every step of the spike in order and
//! writes `result.json` atomically, once, on every way out: the normal
//! end, SIGTERM from `launchctl bootout`, and its own time limit.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use uuid::Uuid;

use super::audit::{self, MachO};
use super::plan::{Layout, Plan, PlanArtifact, Redaction};
use super::process::{self, CleanEnv, LOG_TAIL_BYTES, Server};
use super::providers::{self, Ctx, Recipe};
use super::result::{
    ExecutableSignature, Launch, Libraries, LogTail, Probe, Quarantine, SpikeResult, SpikeStatus,
    Step, StepStatus, Stop,
};
use super::{SOCKET_PREFIX, archive, quarantine, signing};
use crate::runtime::macho;

/// How long the server may take to become ready (AC-5).
const READY_TIMEOUT: Duration = Duration::from_secs(120);
/// How long the server may take to stop after the request (AC-5).
const STOP_TIMEOUT: Duration = Duration::from_secs(60);
/// Time between readiness attempts.
const READY_POLL: Duration = Duration::from_millis(250);

/// What the signal handler and the watchdog need to end the run.
struct Finish {
    /// The result as of the last finished step.
    snapshot: Option<SpikeResult>,
    /// Where it goes.
    path: PathBuf,
    /// What every string is cleaned of.
    redactions: Vec<Redaction>,
    /// The socket folder to delete.
    socket_dir: Option<PathBuf>,
    /// Set once the result is on disk.
    written: bool,
}

static FINISH: Mutex<Option<Finish>> = Mutex::new(None);

pub fn run(args: &[&str]) -> Result<ExitCode> {
    let plan_path = match args {
        ["--plan", path] => PathBuf::from(path),
        _ => bail!("use `cargo xtask runtime spike-worker --plan <file>`; launchd starts it"),
    };
    if std::os::unix::process::parent_id() != 1 {
        bail!("spike-worker only runs under launchd: its parent must be launchd (PID 1)");
    }
    let plan = Plan::read(&plan_path)?;
    let layout = plan.layout();
    let mut result = SpikeResult::new(&plan.artifact.id, &plan.artifact.sha256);
    result.launch = Some(Launch {
        domain: plan.domain.clone(),
        parent_is_launchd: true,
        controlling_tty: has_controlling_tty(),
    });
    *FINISH.lock().unwrap_or_else(PoisonError::into_inner) = Some(Finish {
        snapshot: Some(result.clone()),
        path: layout.result(),
        redactions: plan.redactions.clone(),
        socket_dir: None,
        written: false,
    });
    ctrlc::set_handler(|| end_early("worker", "terminated by a signal before the spike finished"))
        .context("cannot install the SIGTERM handler")?;
    let limit = Duration::from_secs(plan.worker_timeout_secs);
    thread::spawn(move || {
        thread::sleep(limit);
        end_early(
            "worker",
            &format!("timed out after {} minutes", limit.as_secs() / 60),
        );
    });

    let mut spike = Spike {
        artifact: plan.artifact.clone(),
        plan,
        layout,
        result,
        note: None,
        non_system: BTreeSet::new(),
    };
    spike.run();
    process::kill_all_groups();
    finish(Some(spike.result));
    Ok(ExitCode::SUCCESS)
}

/// The signal handler and the watchdog: kill every provider group, write
/// the last snapshot as `FAIL`, and exit.
fn end_early(step: &str, reason: &str) {
    process::kill_all_groups();
    let snapshot = FINISH
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
        .and_then(|finish| finish.snapshot.take());
    if let Some(mut result) = snapshot {
        result.fail(step, reason);
        finish(Some(result));
    }
    std::process::exit(1);
}

/// Writes the result once and deletes the socket folder.
fn finish(result: Option<SpikeResult>) {
    let mut guard = FINISH.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(state) = guard.as_mut() else {
        return;
    };
    if let Some(dir) = state.socket_dir.take() {
        // Ignored: the cleanup routine deletes what is left.
        let _ = fs::remove_dir_all(dir);
    }
    if state.written {
        return;
    }
    if let Some(result) = result
        && let Err(err) = result.write(&state.path, &state.redactions)
    {
        eprintln!("spike-worker: {err:#}");
        return;
    }
    state.written = true;
}

/// Keeps the snapshot the signal handler would write current.
fn publish(result: &SpikeResult) {
    if let Some(state) = FINISH
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
        && !state.written
    {
        state.snapshot = Some(result.clone());
    }
}

fn remember_socket_dir(dir: &Path) {
    if let Some(state) = FINISH
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        state.socket_dir = Some(dir.to_path_buf());
    }
}

/// True when the worker can open `/dev/tty`; under launchd it fails with
/// ENXIO because there is no controlling terminal.
fn has_controlling_tty() -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .is_ok()
}

/// A step failed; its reason is in the result.
struct Halt;

struct Spike {
    plan: Plan,
    artifact: PlanArtifact,
    layout: Layout,
    result: SpikeResult,
    /// The detail the current step adds to its entry.
    note: Option<String>,
    /// File names of the non system libraries the static audit found.
    non_system: BTreeSet<String>,
}

impl Spike {
    fn run(&mut self) {
        self.result.quarantine = Some(Quarantine {
            outcome: "skipped".to_owned(),
            detail: "the spike failed before the quarantine probe".to_owned(),
        });
        if self.steps().is_ok() {
            self.result.status = SpikeStatus::Pass;
        }
    }

    /// Runs `body` as the step `name`: its time and outcome go into the
    /// result, and a failure ends the spike with `<name>: <reason>`.
    fn step<T>(
        &mut self,
        name: &str,
        body: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T, Halt> {
        let started = Instant::now();
        self.note = None;
        let outcome = body(self);
        let duration_ms = millis(started.elapsed());
        let (status, detail, value) = match outcome {
            Ok(value) => (StepStatus::Pass, self.note.take(), Ok(value)),
            Err(err) => {
                let reason = format!("{err:#}");
                self.result.fail(name, &reason);
                (StepStatus::Fail, Some(reason), Err(Halt))
            }
        };
        self.result.steps.push(Step {
            name: name.to_owned(),
            status,
            duration_ms,
            detail,
        });
        publish(&self.result);
        value
    }

    fn steps(&mut self) -> Result<(), Halt> {
        let artifact = self.artifact.clone();
        self.step("launch", |s| {
            if s.result
                .launch
                .as_ref()
                .is_some_and(|launch| launch.controlling_tty)
            {
                bail!("the worker has a controlling terminal");
            }
            s.note = Some(format!(
                "{}, parent launchd, no controlling terminal",
                s.plan.domain
            ));
            Ok(())
        })?;
        self.step("hash", |s| {
            archive::check_digest(&s.plan.cache_file, &artifact.sha256, artifact.size)
        })?;
        let root = self.step("extract", |s| {
            let root = archive::extract(
                &artifact,
                &s.plan.cache_file,
                &s.layout.extract_dir(&artifact.id),
            )?;
            for executable in &artifact.executables {
                if !root.join(executable).is_file() {
                    bail!("the artifact has no executable {executable}");
                }
            }
            Ok(root)
        })?;
        self.step("archive report", |s| {
            let report = archive::report(&root)?;
            s.note = Some(format!(
                "{} files, {} internal links",
                report.files, report.symlinks_internal
            ));
            s.result.archive = Some(report);
            Ok(())
        })?;
        let machos = self.step("static audit", |s| s.static_audit(&root))?;
        self.step("closure", |s| s.closure())?;
        self.step("signing", |s| s.signing(&root, &machos))?;
        self.step("min_os", |s| {
            let minimum = macho::min_os(&root.join(artifact.main_executable()))?;
            s.note = Some(minimum.clone());
            s.result.min_os_measured = Some(minimum);
            Ok(())
        })?;
        let recipe = self.step("recipe", |_| {
            providers::recipe(&artifact.provider).with_context(|| {
                format!("the spike has no steps for provider {}", artifact.provider)
            })
        })?;
        let mut ctx = self.step("init", |s| s.init(&root, &*recipe))?;
        let started = self.step("start", |_| {
            let command = recipe.start(&mut ctx)?;
            Server::spawn("server", command, &ctx.run_dir)
        });
        let mut server = match started {
            Ok(server) => server,
            Err(halt) => {
                self.keep_logs(&*recipe, &ctx, None);
                return Err(halt);
            }
        };
        let running = self.running(&*recipe, &ctx, &mut server);
        if running.is_err() {
            server.kill();
        }
        self.keep_logs(&*recipe, &ctx, Some(&server));
        running?;
        if self.plan.quarantine {
            self.step("quarantine", |s| {
                let probe = quarantine::probe(
                    &root,
                    &s.layout.quarantine_dir(&artifact.id),
                    artifact.main_executable(),
                    recipe.version_args(),
                    &ctx.env,
                )?;
                s.note = Some(format!("{}: {}", probe.outcome, probe.detail));
                s.result.quarantine = Some(probe);
                Ok(())
            })?;
        } else {
            self.result.quarantine = Some(Quarantine {
                outcome: "skipped".to_owned(),
                detail: match &self.plan.quarantine_recorded_in {
                    Some(folder) => format!("already recorded in {folder}"),
                    None => "not requested by the plan".to_owned(),
                },
            });
        }
        Ok(())
    }

    /// The steps while the server runs; the caller kills it on failure.
    fn running(&mut self, recipe: &dyn Recipe, ctx: &Ctx, server: &mut Server) -> Result<(), Halt> {
        self.step("ready", |s| {
            loop {
                if let Some(status) = server.try_exit()? {
                    bail!(
                        "the server exited with {} before it was ready",
                        process::describe(status)
                    );
                }
                if recipe.ready(ctx)? {
                    break;
                }
                if server.started.elapsed() >= READY_TIMEOUT {
                    bail!(
                        "the server was not ready within {} s",
                        READY_TIMEOUT.as_secs()
                    );
                }
                thread::sleep(READY_POLL);
            }
            s.note = Some(format!(
                "ready {} ms after the start",
                millis(server.started.elapsed())
            ));
            Ok(())
        })?;
        let expected = self.artifact.version.clone();
        self.step("version", |s| {
            let raw = recipe.version(ctx)?;
            s.result.raw_version = Some(raw.trim().to_owned());
            let observed = recipe
                .normalize(&raw)
                .with_context(|| format!("cannot read a version from `{}`", raw.trim()))?;
            s.result.observed_version = Some(observed.clone());
            if observed != expected {
                bail!("observed version {observed} differs from the inventory version {expected}");
            }
            s.note = Some(observed);
            Ok(())
        })?;
        self.step("probes", |s| {
            let probes = recipe.probes(ctx);
            let failed: Vec<String> = probes
                .iter()
                .filter(|probe| probe.status == StepStatus::Fail)
                .map(|probe| format!("{} ({})", probe.name, probe.detail))
                .collect();
            s.note = Some(format!("{} probe(s)", probes.len()));
            s.result.closure_probes.extend(probes);
            if !failed.is_empty() {
                bail!("probe failed: {}", failed.join("; "));
            }
            Ok(())
        })?;
        self.step("runtime audit", |s| {
            let root = &ctx.artifact_root;
            let pids = audit::group_pids(server.pid())?;
            let (runtime, foreign) = audit::runtime_libraries(&pids, root)?;
            let listeners = audit::listeners(&pids)?;
            let libraries = s.result.libraries.get_or_insert_with(Libraries::default);
            libraries.runtime = runtime;
            libraries.foreign.extend(foreign.iter().cloned());
            s.result.listeners = listeners.clone();
            if let Some(path) = foreign.first() {
                bail!("the server loaded a foreign library {path}");
            }
            if let Some(listener) = listeners
                .iter()
                .find(|l| !audit::is_loopback(&l.address) || !ctx.ports.contains(&l.port))
            {
                bail!("unexpected listener {}:{}", listener.address, listener.port);
            }
            s.note = Some(format!("{} listener(s)", listeners.len()));
            Ok(())
        })?;
        self.step("stop", |s| {
            let asked = Instant::now();
            recipe.stop(ctx, server)?;
            let Some(status) = server.wait_for(STOP_TIMEOUT)? else {
                bail!(
                    "the server was still running {} s after the stop request",
                    STOP_TIMEOUT.as_secs()
                );
            };
            let (exit_code, signal) = process::code_and_signal(status);
            let duration_ms = millis(asked.elapsed());
            s.result.stop = Some(Stop {
                exit_code,
                signal,
                duration_ms,
            });
            s.note = Some(format!(
                "{} after {duration_ms} ms",
                process::describe(status)
            ));
            Ok(())
        })
    }

    fn static_audit(&mut self, root: &Path) -> Result<Vec<MachO>> {
        let machos = audit::scan(root)?;
        let found = audit::static_audit(root, &machos, self.artifact.origin == "maintainer")?;
        self.note = Some(format!(
            "{} Mach-O file(s), {} libraries",
            machos.len(),
            found.libraries.len()
        ));
        self.result.libraries = Some(Libraries {
            static_: found.libraries.iter().cloned().collect(),
            runtime: Vec::new(),
            foreign: found.foreign.clone(),
            warnings: found.warnings.clone(),
        });
        self.non_system = found.non_system;
        if let Some(first) = found.foreign.first() {
            bail!("foreign library {first}");
        }
        Ok(machos)
    }

    /// AC-7: the bundled and own dependencies of the inventory are exactly
    /// the non system libraries the static audit found.
    fn closure(&mut self) -> Result<()> {
        let listed: BTreeSet<String> = self
            .artifact
            .dependencies
            .iter()
            .filter(|dependency| matches!(dependency.kind.as_str(), "bundled" | "own"))
            .map(|dependency| dependency.name.clone())
            .collect();
        let missing: Vec<&String> = self.non_system.difference(&listed).collect();
        let extra: Vec<&String> = listed.difference(&self.non_system).collect();
        let ok = missing.is_empty() && extra.is_empty();
        let detail = if ok {
            format!("{} non system libraries, as listed", listed.len())
        } else {
            format!("not in the inventory: {missing:?}; listed but not found: {extra:?}")
        };
        self.result.closure_probes.push(Probe {
            name: "dependencies".to_owned(),
            status: if ok {
                StepStatus::Pass
            } else {
                StepStatus::Fail
            },
            detail: detail.clone(),
        });
        if !ok {
            bail!("the inventory dependencies differ from the audit: {detail}");
        }
        self.note = Some(detail);
        Ok(())
    }

    fn signing(&mut self, root: &Path, machos: &[MachO]) -> Result<()> {
        let mut observed = Vec::new();
        for executable in &self.artifact.executables {
            observed.push((
                executable.clone(),
                signing::observe(&root.join(executable))?,
            ));
        }
        let files: Vec<&Path> = machos.iter().map(|macho| macho.path.as_path()).collect();
        let adhoc = signing::apply_policy(&self.artifact.signing_policy, &files)?;
        self.result.signature = observed
            .into_iter()
            .map(|(executable, seen)| ExecutableSignature {
                executable,
                kind: seen.kind,
                authority: seen.authority,
                team_id: seen.team_id,
                adhoc_signed_at_install: adhoc,
            })
            .collect();
        self.note = Some(if adhoc {
            format!("signed {} Mach-O file(s) ad hoc", files.len())
        } else {
            "kept the existing signatures".to_owned()
        });
        Ok(())
    }

    fn init(&mut self, root: &Path, recipe: &dyn Recipe) -> Result<Ctx> {
        let run_dir = self.layout.run_dir();
        let env = CleanEnv {
            cwd: run_dir.clone(),
            home: run_dir.join("home"),
            tmpdir: run_dir.join("tmp"),
        };
        for dir in [&run_dir, &env.home, &env.tmpdir] {
            process::create_dir(dir)?;
        }
        let socket_dir = if recipe.uses_socket_dir() {
            let dir = PathBuf::from(format!(
                "{SOCKET_PREFIX}{}",
                &Uuid::now_v7().simple().to_string()[24..]
            ));
            fs::create_dir(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("cannot restrict {}", dir.display()))?;
            remember_socket_dir(&dir);
            Some(dir)
        } else {
            None
        };
        let mut ctx = Ctx {
            artifact_root: root.to_path_buf(),
            executables: self.artifact.executables.clone(),
            data_dir: run_dir.join("data"),
            run_dir,
            env,
            socket_dir,
            ports: Vec::new(),
            logs: Vec::new(),
        };
        let outcome = recipe.init(&mut ctx);
        if outcome.is_err() {
            self.keep_logs(recipe, &ctx, None);
        }
        outcome?;
        Ok(ctx)
    }

    /// Keeps the tail of every provider log in the result.
    fn keep_logs(&mut self, recipe: &dyn Recipe, ctx: &Ctx, server: Option<&Server>) {
        let mut tails: Vec<LogTail> = ctx.logs.clone();
        if let Some(server) = server {
            tails.push(LogTail {
                process: server.name.clone(),
                stdout: process::tail(&server.stdout, LOG_TAIL_BYTES),
                stderr: process::tail(&server.stderr, LOG_TAIL_BYTES),
            });
        }
        for (name, path) in recipe.log_files(ctx) {
            if path.is_file() {
                tails.push(LogTail {
                    process: name,
                    stdout: process::tail(&path, LOG_TAIL_BYTES),
                    stderr: String::new(),
                });
            }
        }
        self.result.log_tail = tails;
        publish(&self.result);
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
