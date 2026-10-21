//! A create rig: the supervisor rig with the create work wired in.

use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex, PoisonError};

use stackly_core::catalog::{
    Catalog, CatalogEntry, CatalogLocation, CatalogMeta, CatalogState, ProviderEntry, TileColor,
    TileHint, ensure_source,
};
use stackly_core::instance::create::{
    CreateDraft, CreateHooks, CreateInstanceWork, CreateWorkDeps, INSTANCE_REGISTRATION,
    InstanceRegistrationCompensator, InstanceRemoved, PortRequestDraft, ProviderChoice,
    SourceRecorder, encode_input,
};
use stackly_core::operation::{BeginError, BeginOutcome, BeginRequest, BusyPolicy, Origin};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_core::secret::{SECRET_REF_COMPENSATOR, SecretCaller, SecretRefCompensator};
use stackly_domain::{
    InstanceId, LockKey, OperationId, OperationKind, OperationResource, ProviderSourceId,
    RegistryError, ResourceRevision, Sha256Digest,
};
use stackly_platform::{Clock, SecretValue};

use crate::common::Rig;

/// Records every `instance.removed` the compensator reports.
#[derive(Default)]
pub struct Removed(Mutex<Vec<(InstanceId, ResourceRevision)>>);

impl Removed {
    pub fn all(&self) -> Vec<(InstanceId, ResourceRevision)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl InstanceRemoved for Removed {
    fn removed(&self, instance: InstanceId, revision: ResourceRevision) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((instance, revision));
    }
}

/// Records the catalog source through `ensure_source`.
struct Recorder {
    rig_registry: Arc<dyn stackly_domain::Registry>,
    clock: Arc<dyn Clock>,
    meta: CatalogMeta,
    fail: Mutex<bool>,
}

impl SourceRecorder for Recorder {
    fn ensure(&self) -> Result<ProviderSourceId, RegistryError> {
        if *self.fail.lock().unwrap_or_else(PoisonError::into_inner) {
            return Err(RegistryError::Busy);
        }
        ensure_source(&*self.rig_registry, &*self.clock, &self.meta)
    }
}

pub struct CreateRig {
    pub rig: Rig,
    pub removed: Arc<Removed>,
    pub catalog: CatalogState,
    pub package: Arc<ValidatedPackage>,
    recorder: Arc<Recorder>,
}

/// The fake package under another revision, so its digest is new and the
/// pin writes it.
fn fresh_package(original: &ValidatedPackage, revision: &str) -> ValidatedPackage {
    let mut files = std::collections::BTreeMap::new();
    for (path, bytes) in original.files().iter() {
        let mut bytes = bytes.to_vec();
        if path == "provider.yaml" {
            let text = String::from_utf8(bytes).expect("the manifest is UTF-8");
            bytes = text
                .replacen(
                    "revision: \"0.3.0\"",
                    &format!("revision: \"{revision}\""),
                    1,
                )
                .into_bytes();
        }
        files.insert(path.to_owned(), bytes);
    }
    ValidatedPackage::from_files(PackageFiles::new("fake", files), NameCheck::Skip)
        .expect("the changed package validates")
}

impl CreateRig {
    /// A rig whose catalog offers the fake package under a revision no
    /// instance pinned yet.
    pub async fn new() -> Self {
        let rig = Rig::new().await;
        let package = Arc::new(fresh_package(&rig.package, "0.9.0"));
        let entry = ProviderEntry {
            package: Arc::clone(&package),
            tile: TileHint {
                mark: "F".to_owned(),
                tile: TileColor::Blue,
            },
            notes: Vec::new(),
        };
        let meta = CatalogMeta {
            revision: 1,
            published_at: rig.fake_now(),
            content_sha256: Sha256Digest::from_bytes([1; 32]),
            location: CatalogLocation::Development("/stackly-test-catalog".into()),
            loaded_at: rig.fake_now(),
        };
        let catalog = CatalogState::Loaded(Arc::new(Catalog::for_tests_with_meta(
            meta.clone(),
            &[CatalogEntry::Provider(entry)],
        )));
        // The rig pinned its own revision with plain `create_dir_all`; the
        // folders above it must be private like the ones a pin makes.
        for dir in [
            rig.layout.layout().providers_dir().join("testing"),
            rig.layout.layout().providers_dir().join("testing/fake"),
        ] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .expect("the provider folder can be made private");
        }
        let removed = Arc::new(Removed::default());
        rig.engine.register_compensator(
            INSTANCE_REGISTRATION,
            Arc::new(InstanceRegistrationCompensator::new(
                Arc::clone(&removed) as Arc<dyn InstanceRemoved>
            )),
        );
        rig.engine.register_compensator(
            SECRET_REF_COMPENSATOR,
            Arc::new(SecretRefCompensator::new(Arc::clone(&rig.secrets))),
        );
        let recorder = Arc::new(Recorder {
            rig_registry: Arc::clone(&rig.registry) as Arc<dyn stackly_domain::Registry>,
            clock: Arc::new(rig.fake.clock().clone()),
            meta,
            fail: Mutex::new(false),
        });
        Self {
            rig,
            removed,
            catalog,
            package,
            recorder,
        }
    }

    /// Makes the catalog source impossible to record.
    pub fn fail_source(&self) {
        *self
            .recorder
            .fail
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
    }

    pub fn deps(&self, hooks: CreateHooks) -> Arc<CreateWorkDeps> {
        Arc::new(CreateWorkDeps {
            catalog: self.catalog.clone(),
            probe: Arc::clone(&self.rig.ports) as Arc<dyn stackly_core::supervisor::PortProbe>,
            secrets: Arc::clone(&self.rig.secrets),
            supervisor: self.rig.sup.clone(),
            events: Arc::clone(&self.rig.events)
                as Arc<dyn stackly_core::supervisor::SupervisorEvents>,
            sources: Arc::clone(&self.recorder) as Arc<dyn SourceRecorder>,
            hooks,
        })
    }

    pub fn draft(&self, name: &str) -> CreateDraft {
        CreateDraft {
            name: name.to_owned(),
            display_name: None,
            provider: ProviderChoice {
                name: self.package.key().provider().to_string(),
                revision: None,
            },
            runtime_id: self.rig.runtime.id(),
            endpoints: vec![("http".to_owned(), PortRequestDraft::Auto)],
            settings: serde_json::json!({}),
            secrets: Vec::new(),
            secret_values: Vec::new(),
            start: false,
        }
    }

    /// Begins a create with the given hooks and secret values.
    pub async fn begin(
        &self,
        id: InstanceId,
        draft: CreateDraft,
        values: Vec<(String, SecretValue)>,
        hooks: CreateHooks,
    ) -> Result<OperationId, BeginError> {
        let name = draft.name.parse().expect("a valid instance name");
        let input = encode_input(id, &draft).expect("the input fits the journal");
        let work = CreateInstanceWork::new(
            id,
            draft,
            values,
            SecretCaller::client("cli".parse().expect("a client code"), 501, true),
            self.deps(hooks),
        );
        let outcome = self
            .rig
            .engine
            .begin(BeginRequest {
                kind: OperationKind::CreateInstance,
                resource: OperationResource::Instance(id),
                creates_resource: true,
                extra_locks: vec![LockKey::InstanceName(name)],
                busy: BusyPolicy::Reject,
                expected_revision: None,
                origin: Origin::Background,
                input,
                work: Box::new(work),
                precheck: None,
            })
            .await?;
        match outcome {
            BeginOutcome::Created(view) => Ok(view.operation.id()),
            other => panic!("expected a created operation, got {other:?}"),
        }
    }
}
