//! `stackly daemon stop` (spec 0015, AC-10): asks the daemon to stop, reads
//! the connection to its end, and makes sure no daemon answers any more.

use serde::Serialize;
use stackly_protocol::daemon::ShutdownMode;
use tokio::time::Instant;
use uuid::Uuid;

use super::Deps;
use super::wait::{
    DAEMON_POLL_INTERVAL, DAEMON_STOP_WAIT, daemon_absent, not_up_yet, sleep_or_interrupt,
};
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::envelope::{Warning, warning};
use crate::output::human::{Cell, HumanWriter, Render};

/// The `daemon stop` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopResult {
    /// A daemon was running when the command started.
    pub was_running: bool,
    /// The daemon run that was asked to stop.
    pub server_instance_id: Option<Uuid>,
}

impl Render for StopResult {
    fn render(&self, out: &mut HumanWriter) {
        let text = if self.was_running {
            "stopped"
        } else {
            "was not running"
        };
        out.kv(&[("Daemon", Cell::fixed(text))]);
    }
}

/// Runs `daemon stop`.
pub async fn stop(deps: &mut Deps<'_>) -> CommandOutput<StopResult> {
    let started = Instant::now();
    let deadline = started + DAEMON_STOP_WAIT;
    let shutdown = match deps
        .probe
        .shutdown(ShutdownMode::Stop, DAEMON_STOP_WAIT)
        .await
    {
        Ok(shutdown) => shutdown,
        Err(failure) if daemon_absent(&failure) => {
            return CommandOutput::ok(StopResult {
                was_running: false,
                server_instance_id: None,
            });
        }
        Err(failure) => {
            let result = StopResult {
                was_running: !not_up_yet(&failure),
                server_instance_id: None,
            };
            return CommandOutput::partial(result, failure);
        }
    };
    let result = StopResult {
        was_running: true,
        server_instance_id: Some(shutdown.server_instance_id),
    };
    let timeout = || CliFailure::daemon_wait_timeout("stop", DAEMON_STOP_WAIT);
    if !shutdown.eof {
        return CommandOutput::partial(result, timeout());
    }
    // The connection ended, so the daemon is about to release its lock;
    // the stop is done once nothing answers on the socket.
    let mut warnings: Vec<Warning> = Vec::new();
    loop {
        match deps.probe.attempt().await {
            Err(failure) if daemon_absent(&failure) => break,
            Ok(reachable) if reachable.server_instance_id != shutdown.server_instance_id => {
                warnings.push(Warning::new(
                    warning::DAEMON_RELAUNCHED,
                    "the daemon exited with an error and launchd started it again, so it is running now",
                ));
                break;
            }
            Ok(_) | Err(_) => {}
        }
        let now = Instant::now();
        if now >= deadline {
            return CommandOutput::partial(result, timeout());
        }
        if let Err(failure) =
            sleep_or_interrupt(deps.interrupt, DAEMON_POLL_INTERVAL.min(deadline - now)).await
        {
            return CommandOutput::partial(result, failure);
        }
    }
    let mut output = CommandOutput::ok(result);
    output.warnings = warnings;
    output
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use stackly_protocol::ErrorBody;
    use stackly_protocol::codes::request;

    use super::super::testing::{AfterInterrupt, Rig, ScriptedProbe, missing, run};
    use super::super::wait::{Reachable, Shutdown};
    use super::*;

    async fn stop_with(
        rig: &Rig,
        probe: &ScriptedProbe,
        interrupt: &mut AfterInterrupt,
    ) -> CommandOutput<StopResult> {
        let mut deps = Deps {
            agent: &rig.env,
            probe,
            interrupt,
        };
        stop(&mut deps).await
    }

    fn eof(id: u128) -> Result<Shutdown, CliFailure> {
        Ok(Shutdown {
            server_instance_id: run(id),
            eof: true,
        })
    }

    fn gone_after(seconds: u64) -> ScriptedProbe {
        ScriptedProbe::new(move |_, elapsed| {
            if elapsed >= Duration::from_secs(seconds) {
                Err(missing())
            } else {
                Ok(Reachable {
                    server_instance_id: run(1),
                })
            }
        })
        .on_shutdown(eof(1))
    }

    #[tokio::test(start_paused = true)]
    async fn a_stopped_daemon_is_not_stopped_again_and_exits_0() {
        let rig = Rig::plain();
        let probe = ScriptedProbe::no_daemon();

        let output = stop_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none());
        let result = output.result.expect("a result");
        assert!(!result.was_running);
        assert_eq!(result.server_instance_id, None);
        assert!(rig.fake.calls().is_empty(), "stop never asks the OS");
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_closes_and_goes_away_is_stopped() {
        let rig = Rig::plain();
        let probe = gone_after(2);

        let output = stop_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none());
        let result = output.result.expect("a result");
        assert!(result.was_running);
        assert_eq!(result.server_instance_id, Some(run(1)));
        assert_eq!(probe.shutdown_modes(), [ShutdownMode::Stop]);
        assert!(rig.fake.calls().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_that_stays_open_is_a_timeout_after_20_seconds() {
        let rig = Rig::plain();
        let probe = ScriptedProbe::running(run(1)).on_shutdown(Ok(Shutdown {
            server_instance_id: run(1),
            eof: false,
        }));

        let output = stop_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        let failure = output.failure.expect("the stop timed out");
        assert!(
            failure
                .body
                .is(&stackly_protocol::codes::cli::DAEMON_WAIT_TIMEOUT)
        );
        assert_eq!(failure.body.detail("action"), Some("stop"));
        assert_eq!(failure.exit_code(), 8);
        let result = output.result.expect("a partial result");
        assert!(result.was_running);
        assert_eq!(result.server_instance_id, Some(run(1)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_socket_that_keeps_answering_after_the_eof_times_out() {
        let rig = Rig::plain();
        let probe = ScriptedProbe::running(run(1)).on_shutdown(eof(1));
        let started = tokio::time::Instant::now();

        let output = stop_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert_eq!(output.failure.expect("timeout").exit_code(), 8);
        assert!(started.elapsed() >= DAEMON_STOP_WAIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_run_after_the_eof_is_a_warning() {
        let rig = Rig::plain();
        let probe = ScriptedProbe::running(run(2)).on_shutdown(eof(1));

        let output = stop_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        assert!(output.failure.is_none());
        assert_eq!(output.warnings[0].code, warning::DAEMON_RELAUNCHED);
    }

    #[tokio::test(start_paused = true)]
    async fn an_old_daemon_without_the_method_exits_5_and_still_runs() {
        let rig = Rig::plain();
        let unknown = CliFailure::daemon(
            ErrorBody::new(&request::UNKNOWN_METHOD, "unknown method")
                .with_detail("method", "daemon.shutdown"),
            None,
        );
        let probe = ScriptedProbe::running(run(1)).on_shutdown(Err(unknown));

        let output = stop_with(&rig, &probe, &mut AfterInterrupt::never()).await;

        let failure = output.failure.expect("refused");
        assert_eq!(failure.exit_code(), 5);
        assert!(output.result.expect("a result").was_running);
    }

    #[tokio::test(start_paused = true)]
    async fn ctrl_c_while_the_socket_still_answers_exits_130() {
        let rig = Rig::plain();
        let probe = ScriptedProbe::running(run(1)).on_shutdown(eof(1));
        let mut interrupt = AfterInterrupt::new(Some(Duration::from_secs(2)));

        let output = stop_with(&rig, &probe, &mut interrupt).await;

        let failure = output.failure.expect("interrupted");
        assert_eq!(failure.exit_code(), 130);
        assert_eq!(failure.body.detail("state"), Some("waiting_for_daemon"));
    }
}
