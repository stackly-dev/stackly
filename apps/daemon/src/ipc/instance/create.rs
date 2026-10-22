//! The real `instance.create` (spec 0020, *Operasi `create_instance`*).
//!
//! The handler reads the world once with `gather` on a blocking thread,
//! outside the gate of `begin`; the engine looks for the idempotency key
//! first (a repeated request throws the gather away), and then the
//! precheck runs the pure `evaluate` over what `gather` read. A draft with
//! an error row is refused synchronously with the error of the first
//! failing row, and with every field issue when that code carries
//! `fields` (D7). Otherwise the operation `create_instance` begins, with
//! the work of `stackly-core`: `validate`, `pin`, `secrets`, `layout`,
//! `register`, and `init`, every step reversible.

use std::sync::{Arc, OnceLock};

use stackly_core::catalog::CatalogState;
use stackly_core::instance::create::{
    CreateDeps, CreateInstanceWork, CreateWorkDeps, Evaluation, SystemIds, encode_input,
    evaluate_for, fresh_instance_id, gather,
};
use stackly_core::operation::{BeginRequest, BusyPolicy, Origin, Precheck, PrecheckOutcome};
use stackly_core::secret::SecretService;
use stackly_core::supervisor::{PortProbe, Supervisor, SupervisorEvents};
use stackly_domain::{InstanceName, LockKey, OperationKind, OperationResource};
use stackly_protocol::fingerprint::request_fingerprint;
use stackly_protocol::instance::{CreateInstanceParams, InstanceActionAccepted};
use stackly_protocol::message::Call;
use stackly_storage::SqliteRegistry;

use super::{action_reply, draft, registry_of, unexpected};
use crate::catalog::SourceHandle;
use crate::dispatch::{HandlerFuture, RequestContext};
use crate::ipc::operation::{StartRequest as OperationStart, replay_operation, start_operation};
use crate::ipc::secret::caller;
use crate::map;
use crate::state::DaemonCore;

/// The trial binds of every create and preflight of this daemon: one at a
/// time, so two drafts for one port never see each other's trial socket as
/// another process (spec 0020, *Pemeriksaan port*, the one false alarm of a
/// probe is two probes at the same instant).
#[must_use]
pub(super) fn serialized_probe() -> Arc<dyn PortProbe> {
    use std::net::{IpAddr, SocketAddr};
    use std::sync::{Mutex, PoisonError};

    use stackly_core::supervisor::{BindOptions, BindOutcome, SocketPortProbe};
    use stackly_domain::{Port, Transport};

    /// The one gate all trial binds of the daemon pass.
    static GATE: Mutex<()> = Mutex::new(());

    struct Serialized(SocketPortProbe);

    impl PortProbe for Serialized {
        fn try_bind(
            &self,
            transport: Transport,
            addr: SocketAddr,
            options: BindOptions,
        ) -> BindOutcome {
            let _gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
            self.0.try_bind(transport, addr, options)
        }

        fn ephemeral_port(&self, transport: Transport, host: IpAddr) -> Option<Port> {
            let _gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
            self.0.ephemeral_port(transport, host)
        }
    }

    Arc::new(Serialized(SocketPortProbe::new()))
}

/// What the create work needs from this run: the catalog, the secret
/// service, the supervisor, the events, the source row of the catalog, and
/// the hooks a test hung on it. `None` while the supervisor has no event
/// sink yet, which is never the case once the engine runs.
#[must_use]
pub fn work_deps(
    core: &DaemonCore,
    registry: &SqliteRegistry,
    secrets: Arc<SecretService>,
    supervisor: Supervisor,
    catalog: CatalogState,
) -> Option<Arc<CreateWorkDeps>> {
    let events: Arc<dyn SupervisorEvents> = core.supervision().events()?;
    let sources = Arc::new(SourceHandle::new(
        Arc::clone(core.catalog_source()),
        registry.clone(),
        core.platform().shared_clock(),
        catalog.clone(),
    ));
    Some(Arc::new(CreateWorkDeps {
        catalog,
        probe: serialized_probe(),
        secrets,
        supervisor,
        events,
        sources,
        hooks: core.create_hooks(),
    }))
}

/// The catalog of this run, or `daemon.starting` while it loads.
pub(super) fn catalog_state(
    core: &DaemonCore,
) -> Result<CatalogState, Box<stackly_protocol::ErrorBody>> {
    core.catalog().map(|state| (*state).clone()).ok_or_else(|| {
        Box::new(crate::ipc::operation::starting(
            stackly_protocol::daemon::subsystem::CATALOG,
        ))
    })
}

/// The real `instance.create`.
pub fn create<'a>(
    core: &'a DaemonCore,
    context: &RequestContext,
    params: CreateInstanceParams,
) -> HandlerFuture<'a, InstanceActionAccepted> {
    let context = context.clone();
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let registry = registry_of(core)?;
        let secrets = core.secrets().service()?;
        let catalog = catalog_state(core)?;
        let deps = work_deps(core, &registry, secrets, supervisor, catalog)
            .ok_or_else(|| unexpected("the supervisor has no event sink"))?;

        let fingerprint = request_fingerprint(&Call::InstanceCreate(params.clone()));
        let key = context.idempotency_key.as_deref();
        let start = OperationStart::new("instance.create", key, fingerprint);

        // A repeated request gets the reply it had, not a new refusal
        // (K12): the lookup of the key comes before the gather.
        if let Some(replay) = replay_operation(core, &context, start).await? {
            return Ok(InstanceActionAccepted {
                accepted: Some(replay.accepted),
                already_in_state: false,
            });
        }

        let (draft, values) = draft::from_create(&params);
        let facts = {
            let deps = Arc::clone(&deps);
            let registry = registry.clone();
            let platform = core.platform().clone();
            let prepared = core.prepared().clone();
            let source = core.catalog_source().id();
            let draft = draft.clone();
            tokio::task::spawn_blocking(move || {
                let create = CreateDeps {
                    registry: &registry,
                    catalog: &deps.catalog,
                    source,
                    platform: &platform,
                    prepared: &prepared,
                    probe: &*deps.probe,
                };
                gather(&create, &draft)
            })
            .await
            .map_err(|_| unexpected("the create gather task failed"))?
            .map_err(|error| {
                tracing::error!(%error, "cannot read the facts of a create");
                unexpected("the facts of the create could not be read")
            })?
        };
        let id = fresh_instance_id(&SystemIds, &facts.known_ids);
        let input = encode_input(id, &draft)
            .map_err(|_| unexpected("the create input does not fit the journal"))?;

        // The precheck keeps its evaluation so a refusal can carry every
        // field issue, which `OperationError` has no channel for (D7).
        let refused: Arc<OnceLock<Evaluation>> = Arc::new(OnceLock::new());
        let precheck: Box<dyn Precheck> = {
            let draft = draft.clone();
            let refused = Arc::clone(&refused);
            Box::new(move || {
                let evaluation = evaluate_for(&draft, &facts, id);
                match evaluation.first_error().cloned() {
                    Some(error) => {
                        let _ = refused.set(evaluation);
                        PrecheckOutcome::Reject(Box::new(error))
                    }
                    None => PrecheckOutcome::Proceed,
                }
            })
        };
        let extra_locks: Vec<LockKey> = draft
            .name
            .parse::<InstanceName>()
            .map(LockKey::InstanceName)
            .into_iter()
            .collect();
        let work = CreateInstanceWork::new(id, draft, values, caller(&context), deps);
        let request = move |origin: Origin| BeginRequest {
            kind: OperationKind::CreateInstance,
            resource: OperationResource::Instance(id),
            creates_resource: true,
            extra_locks,
            busy: BusyPolicy::Reject,
            expected_revision: None,
            origin,
            input,
            work: Box::new(work),
            precheck: Some(precheck),
        };
        match start_operation(core, &context, start, request).await {
            Ok(outcome) => Ok(action_reply(outcome)),
            Err(body) => Err(with_issues(body, &refused)),
        }
    })
}

/// `body` with every field issue of the refused evaluation, when `body` is
/// the error of its first failing row and that code carries `fields`.
fn with_issues(
    body: Box<stackly_protocol::ErrorBody>,
    refused: &OnceLock<Evaluation>,
) -> Box<stackly_protocol::ErrorBody> {
    let Some(evaluation) = refused.get() else {
        return body;
    };
    match evaluation.first_error() {
        Some(error) if error.carries_fields() && error.code() == body.code() => {
            Box::new(body.with_fields(map::create::field_errors(&evaluation.field_issues)))
        }
        _ => body,
    }
}
