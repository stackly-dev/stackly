//! The binary side of `daemon.shutdown` (spec 0015 AC-6): a `stop` request
//! makes `stacklyd` exit 0 after the stop order, and a `restart` request to
//! a daemon that was started by hand is refused with
//! `daemon.restart_unavailable` while the process keeps running.
#![cfg(target_os = "macos")]

mod common;

use std::time::Duration;

use common::{client, runtime, start_daemon};
use stackly_protocol::codes::daemon;
use stackly_protocol::daemon::{ShutdownMode, ShutdownParams};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;

#[test]
fn a_stop_request_ends_the_process_with_exit_0() {
    let root = TestRoot::new().unwrap();
    let daemon = start_daemon(&root);
    let socket = daemon.socket().to_path_buf();

    let reply = runtime().block_on(async {
        let (client, _) = client(&socket).await;
        client
            .call(Call::DaemonShutdown(ShutdownParams {
                mode: ShutdownMode::Stop,
            }))
            .await
    });

    match reply {
        Ok(Reply::DaemonShutdown(accepted)) => assert_eq!(accepted.mode, ShutdownMode::Stop),
        other => panic!("expected an accepted shutdown, got {other:?}"),
    }
    let status = daemon.wait_exit(Duration::from_secs(15)).unwrap();
    assert_eq!(status.code(), Some(0), "{status:?}");
}

#[test]
fn a_restart_request_to_a_manual_daemon_is_refused_and_the_process_keeps_running() {
    let root = TestRoot::new().unwrap();
    let mut daemon = start_daemon(&root);
    let socket = daemon.socket().to_path_buf();

    let reply = runtime().block_on(async {
        let (client, _) = client(&socket).await;
        client
            .call(Call::DaemonShutdown(ShutdownParams {
                mode: ShutdownMode::Restart,
            }))
            .await
    });

    let error = reply.expect_err("a manual daemon refuses a restart");
    assert!(error.is(&daemon::RESTART_UNAVAILABLE), "{error:?}");
    assert_eq!(error.detail("launchContext"), Some("manual"));
    std::thread::sleep(Duration::from_millis(500));
    assert!(daemon.try_status().unwrap().is_none(), "the daemon exited");
    let status = daemon.stop().unwrap();
    assert_eq!(status.code(), Some(0));
}
