//! The site every create test shares: a fake platform and registry with a
//! provider package and an installed runtime.

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use stackly_core::catalog::{
    Catalog, CatalogEntry, CatalogState, ProviderEntry, TileColor, TileHint,
};
use stackly_core::instance::create::{
    Check, CheckStatus, CreateDeps, CreateDraft, CreateFacts, Evaluation, PortRequestDraft,
    ProviderChoice, SecretSourceKind, evaluate, gather,
};
use stackly_core::provider::{ArtifactStatus, NameCheck, ValidatedPackage, read_package};
use stackly_core::supervisor::fixture::install_fake_runtime;
use stackly_core::supervisor::{BindOptions, BindOutcome, PortProbe};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{
    Instance, Port, PortRequest, ProviderRevisionRecord, Registry, Runtime, RuntimeId, Transport,
    WriteBatch, WriteOp,
};
use stackly_platform::{Clock, Platform};
use stackly_testkit::{FakePlatform, FakeRegistry, InstanceBuilder, TestRoot};

/// A trial bind that answers from a table and records what it was asked.
#[derive(Default)]
pub struct ScriptedProbe {
    answers: Mutex<HashMap<(SocketAddr, bool), BindOutcome>>,
    asked: Mutex<Vec<(SocketAddr, bool)>>,
}

impl ScriptedProbe {
    /// Makes every port from `from` to `to` on `host` answer `outcome` for a
    /// bind with `SO_REUSEADDR`, as the exact address check does.
    pub fn block(&self, host: IpAddr, from: u16, to: u16, outcome: BindOutcome) {
        for port in from..=to {
            self.answer(SocketAddr::new(host, port), true, outcome);
        }
    }

    /// Makes a bind of `addr` with `reuse_addr` answer `outcome`.
    pub fn answer(&self, addr: SocketAddr, reuse_addr: bool, outcome: BindOutcome) {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((addr, reuse_addr), outcome);
    }

    /// Every bind asked so far.
    pub fn asked(&self) -> Vec<(SocketAddr, bool)> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl PortProbe for ScriptedProbe {
    fn try_bind(
        &self,
        _transport: Transport,
        addr: SocketAddr,
        options: BindOptions,
    ) -> BindOutcome {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((addr, options.reuse_addr));
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(addr, options.reuse_addr))
            .copied()
            .unwrap_or(BindOutcome::Free)
    }

    fn ephemeral_port(&self, _transport: Transport, _host: IpAddr) -> Option<Port> {
        None
    }
}

pub struct Site {
    /// Keeps the test root alive.
    _root: TestRoot,
    pub fake: FakePlatform,
    pub platform: Platform,
    pub prepared: PreparedLayout,
    pub registry: FakeRegistry,
    pub probe: ScriptedProbe,
    pub catalog: CatalogState,
    pub package: Arc<ValidatedPackage>,
    pub runtime: Runtime,
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the target folder can be made");
    for entry in fs::read_dir(from).expect("the source folder reads") {
        let entry = entry.expect("an entry reads");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the type reads").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a file copies");
        }
    }
}

/// Writes every program the first artifact of the package lists into the
/// runtime folder, as executable files.
fn make_programs(prepared: &PreparedLayout, package: &ValidatedPackage, runtime: &Runtime) {
    use std::os::unix::fs::PermissionsExt;
    let Some(artifact) = package.versions().artifacts().first() else {
        return;
    };
    let executables = match &artifact.status {
        ArtifactStatus::Available { executables, .. } | ArtifactStatus::Fixture { executables } => {
            executables.clone()
        }
        _ => Vec::new(),
    };
    let stackly_domain::InstallMode::Managed { artifact_sha256 } = runtime.install() else {
        return;
    };
    let dir = prepared.layout().runtime_dir(
        runtime.software(),
        runtime.version(),
        runtime.target(),
        artifact_sha256,
    );
    for executable in executables {
        let path = dir.join(executable);
        fs::create_dir_all(path.parent().expect("a program has a folder"))
            .expect("the program folder can be made");
        fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("the program can be written");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("the program can be made executable");
    }
}

pub fn provider_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../providers")
        .join(name)
}

impl Site {
    /// A site with the `fake` provider and its runtime installed.
    pub fn new() -> Self {
        Self::with_provider("fake")
    }

    pub fn with_provider(name: &str) -> Self {
        Self::with_provider_edit(name, |_| {})
    }

    /// A site whose copy of the provider package `edit` changed first.
    pub fn with_provider_edit(name: &str, edit: impl FnOnce(&Path)) -> Self {
        let root = TestRoot::new().expect("a test root can be made");
        let fake = FakePlatform::new(&root);
        let platform = fake.platform();
        let prepared = prepare_layout(&platform).expect("the fake roots prepare");
        let registry = FakeRegistry::new(Arc::new(fake.clock().clone()), fake.clock().now());
        let copy = root.path().join("input").join(name);
        copy_dir(&provider_dir(name), &copy);
        edit(&copy);
        let files = read_package(platform.fs(), &copy).expect("the package reads");
        let package =
            Arc::new(ValidatedPackage::from_files(files, NameCheck::Skip).expect("it validates"));
        let entry = ProviderEntry {
            package: Arc::clone(&package),
            tile: TileHint {
                mark: "F".to_owned(),
                tile: TileColor::Blue,
            },
            notes: Vec::new(),
        };
        let catalog =
            CatalogState::Loaded(Arc::new(Catalog::for_tests(&[CatalogEntry::Provider(
                entry,
            )])));
        let server = root.path().join("stand-in-server");
        fs::write(&server, b"#!/bin/sh\nexit 0\n").expect("the stand in writes");
        let (runtime, artifact) =
            install_fake_runtime(prepared.layout(), &package, &server, fake.clock().now())
                .unwrap_or_else(|error| panic!("the runtime installs: {error}"));
        make_programs(&prepared, &package, &runtime);
        registry
            .commit(WriteBatch::new(vec![
                WriteOp::InsertArtifact(artifact),
                WriteOp::InsertRuntime(runtime.clone()),
                WriteOp::InsertProviderRevision(ProviderRevisionRecord::new(
                    package.provider_revision().clone(),
                    None,
                    None,
                    None,
                )),
            ]))
            .expect("the runtime commits");
        Self {
            _root: root,
            fake,
            platform,
            prepared,
            registry,
            probe: ScriptedProbe::default(),
            catalog,
            package,
            runtime,
        }
    }

    pub fn deps(&self) -> CreateDeps<'_> {
        CreateDeps {
            registry: &self.registry,
            catalog: &self.catalog,
            source: None,
            platform: &self.platform,
            prepared: &self.prepared,
            probe: &self.probe,
        }
    }

    /// A draft for the site's provider and runtime.
    pub fn draft(&self, name: &str) -> CreateDraft {
        CreateDraft {
            name: name.to_owned(),
            display_name: None,
            provider: ProviderChoice {
                name: self.package.key().provider().to_string(),
                revision: None,
            },
            runtime_id: self.runtime.id(),
            endpoints: Vec::new(),
            settings: serde_json::json!({}),
            secrets: Vec::new(),
            secret_values: Vec::new(),
            start: false,
        }
    }

    /// Like [`Site::evaluate`], with `probe` binding instead of the script.
    pub fn evaluate_with_probe(&self, probe: &dyn PortProbe, draft: &CreateDraft) -> Evaluation {
        let deps = CreateDeps {
            probe,
            ..self.deps()
        };
        evaluate(draft, &gather(&deps, draft).expect("the fakes can be read"))
    }

    pub fn facts(&self, draft: &CreateDraft) -> CreateFacts {
        gather(&self.deps(), draft).expect("the fakes can be read")
    }

    pub fn evaluate(&self, draft: &CreateDraft) -> Evaluation {
        evaluate(draft, &self.facts(draft))
    }

    /// Commits an instance of the site's provider and runtime.
    pub fn add_instance(&self, build: impl FnOnce(InstanceBuilder) -> InstanceBuilder) -> Instance {
        let instance = build(InstanceBuilder::new(&self.runtime, self.package.key())).build();
        self.registry
            .commit(WriteBatch::new(vec![WriteOp::InsertInstance(
                instance.clone(),
            )]))
            .expect("the instance commits");
        instance
    }
}

pub fn row<'a>(evaluation: &'a Evaluation, id: &str) -> &'a Check {
    evaluation
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| {
            panic!(
                "no row {id}; rows are {:?}",
                evaluation.checks.iter().map(|c| &c.id).collect::<Vec<_>>()
            )
        })
}

pub fn has_row(evaluation: &Evaluation, id: &str) -> bool {
    evaluation.checks.iter().any(|check| check.id == id)
}

pub fn error_codes(evaluation: &Evaluation) -> Vec<&'static str> {
    evaluation
        .checks
        .iter()
        .filter(|check| check.status == CheckStatus::Error)
        .filter_map(|check| check.error.as_ref().map(|error| error.code()))
        .collect()
}

pub fn paths(evaluation: &Evaluation) -> Vec<(&str, &'static str)> {
    evaluation
        .field_issues
        .iter()
        .map(|issue| (issue.path.as_str(), field_code(issue.code)))
        .collect()
}

fn field_code(code: stackly_core::instance::create::FieldCode) -> &'static str {
    use stackly_core::instance::create::FieldCode as C;
    match code {
        C::InvalidValue => "invalid_value",
        C::Required => "required",
        C::TooShort => "too_short",
        C::TooLong => "too_long",
        C::InvalidFormat => "invalid_format",
        C::OutOfRange => "out_of_range",
        C::TooMany => "too_many",
        C::UnknownValue => "unknown_value",
        _ => "other",
    }
}

pub fn fixed(name: &str, port: u16) -> (String, PortRequestDraft) {
    (name.to_owned(), PortRequestDraft::Fixed(port))
}

pub fn auto(name: &str) -> (String, PortRequestDraft) {
    (name.to_owned(), PortRequestDraft::Auto)
}

pub fn source(field: &str, kind: SecretSourceKind) -> (String, SecretSourceKind) {
    (field.to_owned(), kind)
}

pub fn unused_runtime() -> RuntimeId {
    RuntimeId::generate()
}

#[allow(dead_code)]
pub fn request(port: u16) -> PortRequest {
    PortRequest::Fixed(Port::new(port).expect("a valid port"))
}

/// Commits `runtime` with its artifact and returns its ID.
pub fn commit_runtime(site: &Site, runtime: Runtime) -> RuntimeId {
    let id = runtime.id();
    let digest = match runtime.install() {
        stackly_domain::InstallMode::Managed { artifact_sha256 } => *artifact_sha256,
        stackly_domain::InstallMode::External { .. } => panic!("a managed runtime"),
    };
    let artifact = stackly_domain::ArtifactRecord::new(
        digest,
        runtime.software().clone(),
        runtime.version().clone(),
        runtime.target(),
        None,
        None,
    );
    site.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertArtifact(artifact),
            WriteOp::InsertRuntime(runtime),
        ]))
        .expect("the runtime commits");
    id
}
