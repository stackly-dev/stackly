//! A fake Mac to plan on: the test catalog, a fake registry with the fake
//! runtime installed, and trial binds that answer from a table.

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use stackly_core::catalog::{Catalog, CatalogState};
use stackly_core::environment::{
    InstanceView, Plan, PlanDeps, PlanInputs, Validation, plan, validate,
};
use stackly_core::instance::create::CreateDeps;
use stackly_core::supervisor::fixture::install_fake_runtime;
use stackly_core::supervisor::{BindOptions, BindOutcome, PortProbe};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{
    Endpoint, EnvironmentId, HealthState, Instance, InstanceLifecycle, IpEndpoint,
    ManifestLocation, Port, PortRequest, ProviderRevisionRecord, ReadinessState, Registry, Runtime,
    Transport, WriteBatch, WriteOp,
};
use stackly_platform::{Clock, HostFacts, Platform};
use stackly_testkit::{FakePlatform, FakeRegistry, InstanceBuilder, TestRoot};

use crate::common::{catalog, package};

/// Trial binds from a table; every other address is free.
#[derive(Default)]
pub struct Probe {
    taken: Mutex<HashMap<SocketAddr, BindOutcome>>,
}

impl Probe {
    pub fn take(&self, port: u16) {
        self.taken
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
                BindOutcome::InUse,
            );
    }
}

impl PortProbe for Probe {
    fn try_bind(&self, _: Transport, addr: SocketAddr, _: BindOptions) -> BindOutcome {
        self.taken
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&addr)
            .copied()
            .unwrap_or(BindOutcome::Free)
    }

    fn ephemeral_port(&self, _: Transport, _: IpAddr) -> Option<Port> {
        None
    }
}

pub struct Site {
    pub root: TestRoot,
    pub fake: FakePlatform,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    pub registry: FakeRegistry,
    pub probe: Probe,
    pub catalog: Arc<Catalog>,
    pub state: CatalogState,
    pub runtime: Runtime,
    pub fake_key: stackly_domain::ProviderRevisionKey,
    /// What the supervisor says, by instance.
    pub views: Vec<InstanceView>,
}

impl Site {
    pub fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).expect("the fake roots prepare");
        let registry = FakeRegistry::new(Arc::new(fake.clock().clone()), fake.clock().now());
        let fake_package = package("fake");
        let server = root.path().join("stand-in-server");
        fs::write(&server, b"#!/bin/sh\nexit 0\n").expect("the stand in writes");
        let (runtime, artifact) = install_fake_runtime(
            prepared.layout(),
            &fake_package,
            &server,
            fake.clock().now(),
        )
        .unwrap_or_else(|error| panic!("the runtime installs: {error}"));
        registry
            .commit(WriteBatch::new(vec![
                WriteOp::InsertArtifact(artifact),
                WriteOp::InsertRuntime(runtime.clone()),
                WriteOp::InsertProviderRevision(ProviderRevisionRecord::new(
                    fake_package.provider_revision().clone(),
                    None,
                    None,
                    None,
                )),
            ]))
            .expect("the runtime commits");
        let catalog = Arc::new(catalog());
        Self {
            root,
            fake,
            platform,
            prepared,
            registry,
            probe: Probe::default(),
            state: CatalogState::Loaded(Arc::clone(&catalog)),
            catalog,
            runtime,
            fake_key: fake_package.key().clone(),
            views: Vec::new(),
        }
    }

    /// Commits a fake instance named `name` with `http` on `port`, and
    /// records what the supervisor says of it.
    pub fn instance(&mut self, name: &str, port: u16, lifecycle: InstanceLifecycle) -> Instance {
        let http = IpEndpoint::new(
            "http".parse().expect("an endpoint name"),
            Transport::Tcp,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            PortRequest::Fixed(Port::new(port).expect("a port")),
            None,
        );
        let instance = InstanceBuilder::new(&self.runtime, &self.fake_key)
            .name(name)
            .endpoints(vec![Endpoint::Ip(http)])
            .lifecycle(lifecycle)
            .build();
        self.registry
            .commit(WriteBatch::new(vec![WriteOp::InsertInstance(
                instance.clone(),
            )]))
            .expect("the instance commits");
        self.views.push(InstanceView {
            id: instance.id(),
            name: instance.name().clone(),
            provider: self.fake_key.provider().clone(),
            runtime_version: self.runtime.version().clone(),
            runtime_line: self.runtime.line().clone(),
            lifecycle,
            initialized: lifecycle != InstanceLifecycle::Uninitialized,
            readiness: if lifecycle == InstanceLifecycle::Running {
                ReadinessState::Ready
            } else {
                ReadinessState::Unknown
            },
            health: HealthState::Unknown,
            blocker: None,
            detail: None,
            endpoints: vec![(
                "http".parse().expect("an endpoint name"),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                Some(Port::new(port).expect("a port")),
            )],
        });
        instance
    }

    pub fn host(&self) -> HostFacts {
        self.platform.info().host_facts()
    }

    pub fn location(&self, file: &str) -> ManifestLocation {
        ManifestLocation::new(self.root.path().join("project").join(file))
            .expect("an absolute location")
    }

    pub fn validate(&self, bytes: &[u8]) -> Validation {
        validate(bytes, &self.catalog)
    }

    pub fn plan_at(
        &self,
        bytes: &[u8],
        location: &ManifestLocation,
        registered: Option<EnvironmentId>,
    ) -> Plan {
        let validation = self.validate(bytes);
        let host = self.host();
        let deps = PlanDeps {
            create: CreateDeps {
                registry: &self.registry,
                catalog: &self.state,
                source: None,
                platform: &self.platform,
                prepared: &self.prepared,
                probe: &self.probe,
            },
            catalog: &self.catalog,
            host: &host,
            instances: &self.views,
        };
        plan(
            &deps,
            &PlanInputs {
                manifest: &validation,
                location,
                registered,
                lock: None,
                overrides: None,
            },
        )
        .expect("the plan reads")
    }

    pub fn plan(&self, bytes: &[u8]) -> Plan {
        self.plan_at(bytes, &self.location("stackly.yaml"), None)
    }
}
