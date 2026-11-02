//! `RestartHarness`: one test root that two daemons serve one after the
//! other, as launchd does for a planned restart. It holds the daemon lock
//! of the root, starts a daemon with the `launchd` context, waits for the
//! daemon to ask for its stop order, and starts the next one, so a test can
//! read the registry in between.
#![allow(dead_code, reason = "each scenario uses a part of these helpers")]

use std::path::Path;
use std::time::Duration;

use stackly_core::supervisor::HandoverProof;
use stackly_ipc::CallOptions;
use stackly_platform::LaunchContext;
use stackly_protocol::daemon::{ShutdownAccepted, ShutdownMode, ShutdownParams};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::restart::{PlanState, RestartPlanView, RestartPreview};
use stackly_protocol::{Call, ErrorBody, Reply};
use stacklyd::server::{self, DaemonConfig, DaemonHandle};

use crate::support::{Api, Site, fast, key};

/// The agent label of a test, which writes no launch record.
const LABEL: &str = "dev.stackly.test.agent.restart-plan";

/// A root served by one daemon after another.
pub struct RestartHarness {
    /// The root, with the fake runtime and package.
    pub site: Site,
    config: DaemonConfig,
    handle: Option<DaemonHandle>,
}

impl RestartHarness {
    /// A harness whose daemons hand over only the lines in `proven`; an
    /// empty list makes every running instance `stop_restore`.
    pub fn new(proven: Vec<HandoverProof>) -> Self {
        Self::on(Site::new(), proven)
    }

    /// The same on `site`, for a test that needs another package.
    pub fn on(site: Site, proven: Vec<HandoverProof>) -> Self {
        let mut config = fast();
        config.supervisor.handover_proven = proven;
        Self {
            site,
            config,
            handle: None,
        }
    }

    /// Changes the constants of the daemons this harness starts.
    pub fn tune(&mut self, tune: impl FnOnce(&mut DaemonConfig)) {
        tune(&mut self.config);
    }

    /// Starts a daemon the `launchd` context and returns a client of it.
    pub async fn start(&mut self) -> Api {
        assert!(self.handle.is_none(), "a daemon already runs");
        let deps = self.site.deps(self.config.clone()).with_launch(
            LaunchContext::Launchd {
                label: LABEL.to_owned(),
            },
            None,
        );
        let handle = server::start(deps).await.expect("the server starts");
        handle.started().await;
        self.handle = Some(handle);
        self.site.api().await
    }

    /// The running daemon.
    pub fn daemon(&self) -> &DaemonHandle {
        self.handle.as_ref().expect("a daemon runs")
    }

    /// Whether the daemon asked for its stop order within `wait`.
    pub async fn stop_requested_within(&self, wait: Duration) -> Option<ShutdownMode> {
        tokio::time::timeout(wait, self.daemon().stop_requested())
            .await
            .ok()
    }

    /// Waits until the daemon asks for its stop order, then runs it as the
    /// daemon's main does for a restart.
    pub async fn exit(&mut self) -> ShutdownMode {
        let handle = self.handle.take().expect("a daemon runs");
        let mode = handle.stop_requested().await;
        handle
            .shutdown(ShutdownReason::RestartRequested, std::future::pending())
            .await;
        mode
    }

    /// Stops the daemon the way a person does.
    pub async fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle
                .shutdown(ShutdownReason::Requested, std::future::pending())
                .await;
        }
    }

    /// Waits for the daemon to ask for its stop order, runs it, and starts
    /// the next daemon on the same root.
    pub async fn next_daemon(&mut self) -> Api {
        assert_eq!(self.exit().await, ShutdownMode::Restart);
        self.start().await
    }

    /// The socket of the root.
    pub fn socket(&self) -> std::path::PathBuf {
        self.site.socket()
    }

    /// The folder of the root, for a test that reads files.
    pub fn root(&self) -> &Path {
        self.site.root.path()
    }
}

impl Api {
    /// `daemon.get_info`.
    pub async fn info(&self) -> stackly_protocol::daemon::DaemonInfo {
        match self
            .client
            .call_detailed(Call::DaemonGetInfo, CallOptions::default())
            .await
            .expect("daemon.get_info answers")
        {
            Reply::DaemonGetInfo(info) => info,
            other => panic!("not an info reply: {other:?}"),
        }
    }

    /// Waits until `daemon.get_info` shows a restart plan in `state`.
    pub async fn wait_plan(&self, state: PlanState) -> RestartPlanView {
        let deadline = tokio::time::Instant::now() + crate::support::PATIENCE;
        loop {
            let info = self.info().await;
            if let Some(plan) = info.restart.filter(|plan| plan.state == state) {
                return *plan;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "no restart plan reached {state:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// `daemon.restart_preview`.
    pub async fn preview(&self) -> Result<RestartPreview, Box<ErrorBody>> {
        match self
            .client
            .call_detailed(Call::DaemonRestartPreview, CallOptions::default())
            .await?
        {
            Reply::DaemonRestartPreview(preview) => Ok(preview),
            other => panic!("not a preview reply: {other:?}"),
        }
    }

    /// `daemon.shutdown { mode: restart }` with `key`.
    pub async fn restart_daemon(
        &self,
        key: &str,
        expected_plan_digest: Option<String>,
        force: bool,
    ) -> Result<ShutdownAccepted, Box<ErrorBody>> {
        let params = ShutdownParams {
            mode: ShutdownMode::Restart,
            expected_plan_digest,
            force,
        };
        match self
            .client
            .call_detailed(
                Call::DaemonShutdown(params),
                CallOptions::default().with_idempotency_key(key),
            )
            .await?
        {
            Reply::DaemonShutdown(accepted) => Ok(accepted),
            other => panic!("not a shutdown reply: {other:?}"),
        }
    }

    /// A restart with a fresh key, confirming the digest of the preview.
    pub async fn confirmed_restart(&self) -> Result<ShutdownAccepted, Box<ErrorBody>> {
        let digest = self.preview().await.expect("the preview is read").digest;
        self.restart_daemon(&key(), Some(digest), false).await
    }
}
