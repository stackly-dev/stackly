//! The `exec` probe: the runtime's own executable with the plan's argv and
//! env, stdin from `/dev/null`, whose exit code decides. Its failure
//! detail follows spec 0019 for every `exec` probe: the first non-empty
//! line of the captured stderr tail, else `exit <code>`.

use std::time::Duration;

use stackly_platform::{CapturedOutput, ChildRole, ExitStatus};

use super::{CHECK_FAILED, ProbeContext, ProbeOutcome, TIMEOUT};
use crate::provider::{EnvValue, RunSpec};
use crate::supervisor::helper::{HelperCommand, HelperEnd, run_helper};

/// Runs the probe as a [`ChildRole::Probe`] helper under the shorter of
/// `limit` and its own timeout.
pub(super) async fn run(
    spec: &RunSpec,
    limit: Duration,
    context: ProbeContext<'_>,
) -> ProbeOutcome {
    let redactor = context.redactor;
    let Some(env) = text_env(&spec.command.env) else {
        // Validation refuses a secret anywhere in a probe (spec 0009), so a
        // plan never gets here; the probe fails rather than resolve one.
        return ProbeOutcome::failure(
            CHECK_FAILED,
            "the probe environment holds a secret, which probes never get",
            redactor,
        );
    };
    let command = HelperCommand {
        program: spec.command.program.absolute(),
        args: spec.command.args.clone(),
        env,
        cwd: spec.command.cwd.clone(),
        role: ChildRole::Probe,
        stdin: None,
        lock: None,
    };
    let limit = limit.min(spec.timeout);
    match run_helper(
        context.platform,
        command,
        limit,
        context.config.helper_term_grace,
    )
    .await
    {
        Ok(HelperEnd::Exited(notice)) => match notice.status {
            Some(ExitStatus::Exited(code))
                if u8::try_from(code).is_ok_and(|code| spec.success_exit_codes.contains(&code)) =>
            {
                ProbeOutcome::Ok
            }
            status => {
                let raw = first_stderr_line(notice.captured.as_ref())
                    .unwrap_or_else(|| exit_text(status));
                ProbeOutcome::failure(CHECK_FAILED, &raw, redactor)
            }
        },
        Ok(HelperEnd::TimedOut) => {
            let raw = format!(
                "the check ran past {} ms and was stopped",
                limit.as_millis()
            );
            ProbeOutcome::failure(TIMEOUT, &raw, redactor)
        }
        Err(error) => {
            let raw = format!("the check could not run: {error}");
            ProbeOutcome::failure(CHECK_FAILED, &raw, redactor)
        }
    }
}

/// The plan's env as text, or `None` when it holds a secret.
fn text_env(env: &[(String, EnvValue)]) -> Option<Vec<(String, String)>> {
    env.iter()
        .map(|(name, value)| match value {
            EnvValue::Text(text) => Some((name.clone(), text.clone())),
            EnvValue::Secret(_) => None,
        })
        .collect()
}

/// The first line of the stderr tail with something besides whitespace in
/// it, trimmed, as lossy UTF-8.
fn first_stderr_line(captured: Option<&CapturedOutput>) -> Option<String> {
    let stderr = String::from_utf8_lossy(&captured?.stderr);
    stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

/// `exit <code>`, `signal <n>` for a helper a signal ended, or `exit status
/// unknown` when the watcher reported none.
fn exit_text(status: Option<ExitStatus>) -> String {
    match status {
        Some(ExitStatus::Exited(code)) => format!("exit {code}"),
        Some(ExitStatus::Signaled(signal)) => format!("signal {signal}"),
        None => "exit status unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use stackly_platform::{ProcessError, SecretValue};
    use stackly_testkit::{FakePlatform, FakeProcessSupervision, ProcessOp, TestRoot};
    use tokio::time::Instant;

    use super::*;
    use crate::provider::{Command, ExecutablePath, Stdin};
    use crate::secret::Redactor;
    use crate::supervisor::SupervisorConfig;

    const ATTEMPT: Duration = Duration::from_secs(3);

    struct Rig {
        _root: TestRoot,
        fake: FakePlatform,
        platform: stackly_platform::Platform,
        redactor: Redactor,
        config: SupervisorConfig,
    }

    impl Rig {
        fn new() -> Self {
            let root = TestRoot::new().unwrap();
            let fake = FakePlatform::new(&root);
            let platform = fake.platform();
            Self {
                _root: root,
                fake,
                platform,
                redactor: Redactor::new(),
                config: SupervisorConfig::default(),
            }
        }

        fn processes(&self) -> &FakeProcessSupervision {
            self.fake.processes()
        }

        fn context(&self) -> ProbeContext<'_> {
            ProbeContext {
                platform: &self.platform,
                redactor: &self.redactor,
                config: &self.config,
            }
        }

        /// Runs `spec` in a task and returns it with the PID of its helper.
        async fn start(
            self: &Arc<Self>,
            spec: RunSpec,
        ) -> (tokio::task::JoinHandle<ProbeOutcome>, stackly_domain::Pid) {
            let rig = Arc::clone(self);
            let task = tokio::spawn(async move { run(&spec, ATTEMPT, rig.context()).await });
            let pid = loop {
                if let Some(spawn) = self.processes().spawns().last()
                    && self.processes().count(ProcessOp::WatchExit) > 0
                {
                    break spawn.pid;
                }
                tokio::task::yield_now().await;
            };
            (task, pid)
        }
    }

    fn spec(success: &[u8]) -> RunSpec {
        RunSpec {
            command: Command {
                program: ExecutablePath {
                    runtime_root: PathBuf::from("/runtimes/postgresql"),
                    relative: PathBuf::from("bin/psql"),
                },
                args: vec!["-c".to_owned(), "SELECT 1".to_owned()],
                cwd: PathBuf::from("/instances/a"),
                env: vec![
                    (
                        "PATH".to_owned(),
                        EnvValue::Text("/usr/bin:/bin".to_owned()),
                    ),
                    (
                        "PGDATABASE".to_owned(),
                        EnvValue::Text("postgres".to_owned()),
                    ),
                ],
            },
            stdin: Stdin::Null,
            timeout: ATTEMPT,
            success_exit_codes: success.to_vec(),
        }
    }

    fn output(stderr: &[u8]) -> CapturedOutput {
        CapturedOutput {
            stdout: b"ignored\n".to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    fn detail(outcome: &ProbeOutcome) -> &str {
        outcome.detail().map_or("", |detail| detail.as_str())
    }

    #[tokio::test]
    async fn an_exit_code_in_the_success_list_is_ok_and_the_helper_gets_the_plan() {
        let rig = Arc::new(Rig::new());

        let (task, pid) = rig.start(spec(&[0, 2])).await;
        rig.processes().exit(pid, Some(ExitStatus::Exited(2)));

        assert_eq!(task.await.unwrap(), ProbeOutcome::Ok);
        let spawn = rig.processes().spawns().pop().unwrap();
        assert_eq!(spawn.role, ChildRole::Probe);
        assert_eq!(
            spawn.program,
            PathBuf::from("/runtimes/postgresql/bin/psql")
        );
        assert_eq!(spawn.args, ["-c", "SELECT 1"]);
        assert_eq!(
            spawn.env,
            [
                ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
                ("PGDATABASE".to_owned(), "postgres".to_owned()),
            ]
        );
        assert_eq!(spawn.cwd, PathBuf::from("/instances/a"));
        assert_eq!(spawn.stdin_len, None);
        assert_eq!(spawn.lock, None);
    }

    #[tokio::test]
    async fn another_exit_code_is_check_failed_with_the_first_non_empty_stderr_line() {
        let rig = Arc::new(Rig::new());

        let (task, pid) = rig.start(spec(&[0])).await;
        rig.processes().exit_with_output(
            pid,
            Some(ExitStatus::Exited(2)),
            output(b"\n   \r\npsql: error: connection to server failed\r\nsecond line\n"),
        );

        let outcome = task.await.unwrap();
        assert_eq!(outcome.reason(), &CHECK_FAILED);
        assert_eq!(detail(&outcome), "psql: error: connection to server failed");
    }

    #[tokio::test]
    async fn an_empty_stderr_gives_the_exit_code() {
        let rig = Arc::new(Rig::new());

        let (task, pid) = rig.start(spec(&[0])).await;
        rig.processes()
            .exit_with_output(pid, Some(ExitStatus::Exited(3)), output(b" \n\t\n"));

        let outcome = task.await.unwrap();
        assert_eq!(outcome.reason(), &CHECK_FAILED);
        assert_eq!(detail(&outcome), "exit 3");
    }

    #[tokio::test]
    async fn a_helper_ended_by_a_signal_fails_with_the_signal() {
        let rig = Arc::new(Rig::new());

        let (task, pid) = rig.start(spec(&[0])).await;
        rig.processes().exit(pid, Some(ExitStatus::Signaled(11)));

        let outcome = task.await.unwrap();
        assert_eq!(outcome.reason(), &CHECK_FAILED);
        assert_eq!(detail(&outcome), "signal 11");
    }

    #[tokio::test]
    async fn the_stderr_line_is_redacted_then_cut_to_256_characters() {
        let rig = Arc::new(Rig::new());
        rig.redactor
            .learn(&SecretValue::new("hunter2-hunter2".to_owned()).unwrap());
        let mut line = b"FATAL: password hunter2-hunter2 is wrong ".to_vec();
        line.extend(std::iter::repeat_n(b'z', 400));

        let (task, pid) = rig.start(spec(&[0])).await;
        rig.processes()
            .exit_with_output(pid, Some(ExitStatus::Exited(1)), output(&line));

        let outcome = task.await.unwrap();
        let detail = detail(&outcome);
        assert!(
            detail.starts_with("FATAL: password **** is wrong z"),
            "{detail}"
        );
        assert_eq!(detail.chars().count(), 256);
    }

    #[tokio::test(start_paused = true)]
    async fn a_helper_past_its_limit_gets_term_then_kill_and_times_out() {
        let rig = Arc::new(Rig::new());
        let started = Instant::now();

        let (task, pid) = rig.start(spec(&[0])).await;
        tokio::time::sleep(ATTEMPT + rig.config.helper_term_grace + Duration::from_millis(1)).await;
        let terminations: Vec<_> = rig
            .processes()
            .calls()
            .into_iter()
            .filter(|call| call.op == ProcessOp::TerminateHelperGroup)
            .map(|call| (call.pid, call.force))
            .collect();
        assert_eq!(
            terminations,
            [(Some(pid), Some(false)), (Some(pid), Some(true))]
        );
        rig.processes().exit(pid, Some(ExitStatus::Signaled(9)));

        let outcome = task.await.unwrap();
        assert_eq!(outcome.reason(), &TIMEOUT);
        assert_eq!(
            detail(&outcome),
            "the check ran past 3000 ms and was stopped"
        );
        assert!(started.elapsed() < ATTEMPT + rig.config.helper_term_grace * 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_helper_that_obeys_term_is_never_killed() {
        let rig = Arc::new(Rig::new());

        let (task, pid) = rig.start(spec(&[0])).await;
        tokio::time::sleep(ATTEMPT + Duration::from_millis(1)).await;
        rig.processes().exit(pid, Some(ExitStatus::Signaled(15)));

        let outcome = task.await.unwrap();
        assert_eq!(outcome.reason(), &TIMEOUT);
        let forces: Vec<_> = rig
            .processes()
            .calls()
            .into_iter()
            .filter(|call| call.op == ProcessOp::TerminateHelperGroup)
            .map(|call| call.force)
            .collect();
        assert_eq!(forces, [Some(false)]);
    }

    #[tokio::test(start_paused = true)]
    async fn the_shorter_of_the_attempt_and_the_probe_timeout_applies() {
        let rig = Arc::new(Rig::new());
        let mut short = spec(&[0]);
        short.timeout = Duration::from_millis(400);

        let (task, pid) = rig.start(short).await;
        tokio::time::sleep(Duration::from_millis(401)).await;
        rig.processes().exit(pid, Some(ExitStatus::Signaled(15)));

        let outcome = task.await.unwrap();
        assert_eq!(
            detail(&outcome),
            "the check ran past 400 ms and was stopped"
        );
    }

    #[tokio::test]
    async fn a_spawn_failure_is_check_failed_and_nothing_runs() {
        let rig = Rig::new();
        rig.processes().fail_next(
            ProcessOp::SpawnHelper,
            ProcessError::SpawnFailed {
                kind: std::io::ErrorKind::NotFound,
                errno: 2,
            },
        );

        let outcome = run(&spec(&[0]), ATTEMPT, rig.context()).await;

        assert_eq!(outcome.reason(), &CHECK_FAILED);
        assert!(
            detail(&outcome).starts_with("the check could not run: the spawn failed"),
            "{}",
            detail(&outcome)
        );
        assert!(rig.processes().spawns().is_empty());
    }

    #[tokio::test]
    async fn a_failed_exit_watch_kills_the_helper_and_fails_the_check() {
        let rig = Rig::new();
        rig.processes().fail_next(
            ProcessOp::WatchExit,
            ProcessError::Io {
                op: "kevent",
                path: None,
                kind: std::io::ErrorKind::Other,
                message: "injected".to_owned(),
            },
        );

        let outcome = run(&spec(&[0]), ATTEMPT, rig.context()).await;

        assert_eq!(outcome.reason(), &CHECK_FAILED);
        let forces: Vec<_> = rig
            .processes()
            .calls()
            .into_iter()
            .filter(|call| call.op == ProcessOp::TerminateHelperGroup)
            .map(|call| call.force)
            .collect();
        assert_eq!(forces, [Some(true)]);
    }

    #[tokio::test]
    async fn a_secret_in_the_env_is_refused_without_a_spawn() {
        let rig = Rig::new();
        let mut with_secret = spec(&[0]);
        with_secret.command.env.push((
            "PGPASSWORD".to_owned(),
            EnvValue::Secret(crate::provider::PlanSecret {
                field: "password".parse().unwrap(),
                reference: "local:orders-db/password".parse().unwrap(),
            }),
        ));

        let outcome = run(&with_secret, ATTEMPT, rig.context()).await;

        assert_eq!(outcome.reason(), &CHECK_FAILED);
        assert!(rig.processes().spawns().is_empty());
    }
}
