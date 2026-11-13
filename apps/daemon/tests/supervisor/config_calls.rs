//! The config calls of the socket for the native provider tests (spec 0029,
//! milestone 4): `instance.config.get`, `validate`, `apply`, and `keep_revision`, and
//! `instance.change_endpoints`, with the waits the tests share.

#![allow(
    dead_code,
    reason = "each native test crate that mounts this file calls a part of it"
)]

use std::time::Duration;

use serde_json::Value;
use stackly_ipc::CallOptions;
use stackly_protocol::instance::config::{
    ApplyConfigParams, ChangeEndpointsParams, ChangeEndpointsReply, ConfigApplyAccepted,
    ConfigChangeRequest, ConfigImpact, ConfigValidation, GetConfigParams, InstanceConfigView,
    KeepRevisionParams, ValidateConfigParams,
};
use stackly_protocol::instance::{
    EndpointRequest, InstanceActionAccepted, InstanceSummary, PortRequest, SettingsDocument,
};
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, Reply};
use uuid::Uuid;

use crate::support::{Api, key};

pub fn number(n: u64) -> ConfigRevisionNumber {
    ConfigRevisionNumber::new(n).expect("a revision number")
}

pub fn pid_of(summary: &InstanceSummary) -> u32 {
    summary.process.as_ref().expect("a process runs").pid
}

/// The config methods of the socket, for one test.
pub struct Cfg<'a> {
    pub api: &'a Api,
}

impl Cfg<'_> {
    pub async fn view(&self, id: Uuid) -> InstanceConfigView {
        match self
            .api
            .client
            .call(Call::InstanceConfigGet(GetConfigParams { instance_id: id }))
            .await
            .expect("the config can be read")
        {
            Reply::InstanceConfigGet(view) => *view,
            other => panic!("not a config reply: {other:?}"),
        }
    }

    pub async fn wait_view(
        &self,
        id: Uuid,
        what: &str,
        ready: impl Fn(&InstanceConfigView) -> bool,
    ) -> InstanceConfigView {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            let view = self.view(id).await;
            if ready(&view) {
                return view;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the config of {id} did not get {what}: {view:#?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn validate(
        &self,
        id: Uuid,
        base: u64,
        change: ConfigChangeRequest,
    ) -> ConfigValidation {
        match self
            .api
            .client
            .call(Call::InstanceConfigValidate(ValidateConfigParams {
                instance_id: id,
                base_revision: number(base),
                change,
            }))
            .await
            .expect("the validation answers")
        {
            Reply::InstanceConfigValidate(verdict) => *verdict,
            other => panic!("not a validate reply: {other:?}"),
        }
    }

    pub async fn impact(&self, id: Uuid, base: u64, change: ConfigChangeRequest) -> ConfigImpact {
        let verdict = self.validate(id, base, change).await;
        assert!(verdict.valid, "{verdict:#?}");
        verdict.impact
    }

    pub async fn apply(
        &self,
        id: Uuid,
        base: u64,
        change: ConfigChangeRequest,
    ) -> Result<ConfigApplyAccepted, Box<ErrorBody>> {
        match self
            .api
            .client
            .call_detailed(
                Call::InstanceConfigApply(ApplyConfigParams {
                    instance_id: id,
                    base_revision: number(base),
                    change,
                    expected_revision: None,
                }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceConfigApply(accepted) => Ok(*accepted),
            other => panic!("not an apply reply: {other:?}"),
        }
    }

    /// Applies `change` and returns the operation once it ended, whatever
    /// its state.
    pub async fn apply_final(
        &self,
        id: Uuid,
        base: u64,
        change: ConfigChangeRequest,
    ) -> OperationDetail {
        let accepted = self
            .apply(id, base, change)
            .await
            .expect("the apply is accepted")
            .accepted
            .expect("a change begins an operation");
        self.api.wait_final(accepted.operation.id).await
    }

    pub async fn apply_done(&self, id: Uuid, base: u64, change: ConfigChangeRequest) {
        let detail = self.apply_final(id, base, change).await;
        assert_eq!(
            detail.operation.state,
            OperationState::Succeeded,
            "{detail:?}"
        );
    }

    pub async fn keep(&self, id: Uuid) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        match self
            .api
            .client
            .call_detailed(
                Call::InstanceConfigKeepRevision(KeepRevisionParams { instance_id: id }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceConfigKeepRevision(accepted) => Ok(*accepted),
            other => panic!("not a keep reply: {other:?}"),
        }
    }

    pub async fn change_endpoints(
        &self,
        id: Uuid,
        base: u64,
        endpoint: &str,
        port: PortRequest,
    ) -> Result<ChangeEndpointsReply, Box<ErrorBody>> {
        match self
            .api
            .client
            .call_detailed(
                Call::InstanceChangeEndpoints(ChangeEndpointsParams {
                    instance_id: id,
                    base_revision: number(base),
                    endpoints: vec![EndpointRequest {
                        name: endpoint.to_owned(),
                        port,
                    }],
                    dry_run: false,
                    override_leases: false,
                }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceChangeEndpoints(reply) => Ok(*reply),
            other => panic!("not a change endpoints reply: {other:?}"),
        }
    }
}

/// The settings of the active revision with `edits` laid over them.
pub fn settings_with(view: &InstanceConfigView, edits: Value) -> ConfigChangeRequest {
    let mut map = view.revision.values.as_map().clone();
    for (field, value) in edits.as_object().expect("edits are an object") {
        map.insert(field.clone(), value.clone());
    }
    ConfigChangeRequest::Settings(SettingsDocument::new(map))
}

pub fn error_code(result: Result<impl std::fmt::Debug, Box<ErrorBody>>) -> String {
    result
        .expect_err("the request is refused")
        .code()
        .to_owned()
}
