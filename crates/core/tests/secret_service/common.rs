//! The harness every secret service test shares: the service on
//! `FakePlatform`, whose store is a `FakeSecretStore`, and `FakeRegistry`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use stackly_core::provider::ApplyClass;
use stackly_core::secret::{
    PutOutcome, PutRequest, PutTarget, Redactor, SecretCaller, SecretChange, SecretDeps,
    SecretError, SecretEvents, SecretFieldRule, SecretFieldRules, SecretInput, SecretService,
};
use stackly_domain::{
    AuditEventRecord, AuditQuery, EnvironmentId, Instance, InstanceId, Registry, SecretFieldName,
    SecretRef, SecretReferenceView, SecretUser, WriteBatch, WriteOp,
};
use stackly_platform::{
    Clock, Interaction, Platform, SecretBackend, SecretPresence, SecretStore, SecretStoreError,
    SecretValue, StoreState, WriteMode,
};
use stackly_testkit::{
    EnvironmentBuilder, FakePlatform, FakeRegistry, FakeSecretStore, InstanceBuilder,
    ProviderRevisionBuilder, RuntimeBuilder, TestRoot,
};

/// Records every change the service reports.
#[derive(Debug, Clone, Default)]
pub struct Changes(Arc<Mutex<Vec<SecretChange>>>);

impl Changes {
    pub fn all(&self) -> Vec<SecretChange> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SecretEvents for Changes {
    fn changed(&self, change: &SecretChange) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(change.clone());
    }
}

/// Field rules a test sets per instance and field.
#[derive(Debug, Default)]
pub struct Rules(Mutex<HashMap<(InstanceId, String), SecretFieldRule>>);

impl Rules {
    pub fn set(&self, instance: InstanceId, field: &str, rule: SecretFieldRule) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((instance, field.to_owned()), rule);
    }
}

impl SecretFieldRules for Rules {
    fn rules(&self, instance: &Instance, field: &SecretFieldName) -> Option<SecretFieldRule> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(instance.id(), field.as_str().to_owned()))
            .copied()
    }
}

/// The service on fakes, in its own test root.
pub struct Harness {
    pub _root: TestRoot,
    pub fake: FakePlatform,
    pub platform: Arc<Platform>,
    pub registry: Arc<FakeRegistry>,
    pub redactor: Arc<Redactor>,
    pub changes: Changes,
    pub rules: Arc<Rules>,
    pub service: Arc<SecretService>,
}

impl Harness {
    pub fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root);
        let platform = Arc::new(fake.platform());
        Self::on(root, fake, platform)
    }

    /// The service on `platform`, which may hold another store, such as a
    /// worker in front of the fake one.
    pub fn on(root: TestRoot, fake: FakePlatform, platform: Arc<Platform>) -> Self {
        let registry = Arc::new(FakeRegistry::new(
            Arc::new(fake.clock().clone()),
            fake.clock().now(),
        ));
        let redactor = Arc::new(Redactor::new());
        let changes = Changes::default();
        let rules = Arc::new(Rules::default());
        let service = Arc::new(SecretService::new(SecretDeps {
            registry: Arc::clone(&registry) as Arc<dyn Registry>,
            platform: Arc::clone(&platform),
            redactor: Arc::clone(&redactor),
            fields: Arc::clone(&rules) as Arc<dyn SecretFieldRules>,
            events: Arc::new(changes.clone()),
        }));
        Self {
            _root: root,
            fake,
            platform,
            registry,
            redactor,
            changes,
            rules,
            service,
        }
    }

    /// A second service on the same registry and store, with a fresh
    /// redactor, as the daemon after a restart.
    pub fn restarted(&self) -> Restarted {
        let redactor = Arc::new(Redactor::new());
        let service = SecretService::new(SecretDeps {
            registry: Arc::clone(&self.registry) as Arc<dyn Registry>,
            platform: Arc::clone(&self.platform),
            redactor: Arc::clone(&redactor),
            fields: Arc::clone(&self.rules) as Arc<dyn SecretFieldRules>,
            events: Arc::new(Changes::default()),
        });
        Restarted { service, redactor }
    }

    pub fn store(&self) -> &FakeSecretStore {
        self.fake.secrets()
    }

    /// Stores an instance, with its provider revision and runtime; with
    /// `initialized`, its data is initialized.
    pub fn instance(&self, initialized: bool) -> InstanceId {
        let provider = ProviderRevisionBuilder::new();
        let runtime = RuntimeBuilder::new();
        let built = runtime.clone().build();
        let mut builder = InstanceBuilder::new(&built, &provider.key());
        if initialized {
            builder = builder.data_initialized_at(self.fake.clock().now());
        }
        let instance = builder.build();
        let id = instance.id();
        self.commit(vec![
            WriteOp::InsertProviderRevision(provider.record()),
            WriteOp::InsertArtifact(runtime.artifact()),
            WriteOp::InsertRuntime(built),
            WriteOp::InsertInstance(instance),
        ]);
        id
    }

    pub fn environment(&self) -> EnvironmentId {
        let environment = EnvironmentBuilder::new().build();
        let id = environment.id();
        self.commit(vec![WriteOp::InsertEnvironment(environment)]);
        id
    }

    pub fn commit(&self, ops: Vec<WriteOp>) {
        self.registry
            .commit(WriteBatch::new(ops))
            .expect("a test commit succeeds");
    }

    /// Records that `user` needs each reference for its field.
    pub fn uses(&self, user: SecretUser, uses: &[(&str, &SecretRef)]) {
        let uses: BTreeMap<SecretFieldName, SecretRef> = uses
            .iter()
            .map(|(field, reference)| (field_name(field), (*reference).clone()))
            .collect();
        self.commit(vec![WriteOp::ReplaceSecretUses { user, uses }]);
    }

    /// Sets the rule of an instance field.
    pub fn rule(
        &self,
        instance: InstanceId,
        field: &str,
        min: u64,
        max: Option<u64>,
        apply: Option<ApplyClass>,
    ) {
        self.rules
            .set(instance, field, SecretFieldRule::new(min, max, apply));
    }

    pub fn put(&self, request: PutRequest) -> Result<PutOutcome, SecretError> {
        self.service.put(request, &user())
    }

    /// Stores `text` behind a user reference through `secret.set`.
    pub fn set(&self, reference: &SecretRef, text: &str) -> PutOutcome {
        self.put(PutRequest::new(
            PutTarget::Existing(reference.clone()),
            SecretInput::Value(value(text)),
            WriteMode::Upsert,
        ))
        .expect("a test value is stored")
    }

    pub fn view(&self, reference: &SecretRef) -> Option<SecretReferenceView> {
        self.registry
            .secret_reference(reference)
            .expect("the registry reads")
            .value
    }

    /// Every audit row, oldest first.
    pub fn audit(&self) -> Vec<AuditEventRecord> {
        let mut rows = self
            .registry
            .audit_events(AuditQuery {
                limit: 500,
                before: None,
            })
            .expect("the audit rows read")
            .value
            .items;
        rows.reverse();
        rows
    }
}

/// A second service, as [`Harness::restarted`] makes it.
pub struct Restarted {
    pub service: SecretService,
    pub redactor: Arc<Redactor>,
}

pub fn reference(text: &str) -> SecretRef {
    SecretRef::parse(text).expect("a valid test reference")
}

pub fn value(text: &str) -> SecretValue {
    SecretValue::new(text.to_owned()).expect("a valid test value")
}

pub fn field_name(text: &str) -> SecretFieldName {
    text.parse().expect("a valid test field name")
}

/// An interactive user of the CLI.
pub fn user() -> SecretCaller {
    SecretCaller::client("cli".parse().expect("a client code"), 501, true)
}

/// The reference the daemon makes for a field of an instance.
pub fn instance_reference(instance: InstanceId, purpose: &str) -> SecretRef {
    reference(&format!("local:instance/{instance}/{purpose}"))
}

/// The fake store with failures a test scripts: every read of one
/// reference, or every write after the first `writes_ok`.
#[derive(Debug, Default)]
pub struct ScriptedStore {
    pub inner: FakeSecretStore,
    pub fail_reads_of: Mutex<Option<(SecretRef, SecretStoreError)>>,
    pub writes_ok: Mutex<Option<usize>>,
}

impl ScriptedStore {
    /// A harness whose platform holds this store in front of the fake one.
    pub fn harness() -> (Harness, Arc<Self>) {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root);
        let store = Arc::new(Self {
            inner: fake.secrets().clone(),
            ..Self::default()
        });
        let platform = Arc::new(
            fake.platform()
                .with_secrets(Arc::clone(&store) as Arc<dyn SecretStore>),
        );
        (Harness::on(root, fake, platform), store)
    }
}

impl SecretStore for ScriptedStore {
    fn backend(&self) -> SecretBackend {
        self.inner.backend()
    }

    fn state(&self) -> StoreState {
        self.inner.state()
    }

    fn store_key(&self, reference: &SecretRef) -> String {
        self.inner.store_key(reference)
    }

    fn probe(&self, reference: &SecretRef) -> Result<SecretPresence, SecretStoreError> {
        self.inner.probe(reference)
    }

    fn read(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<SecretValue, SecretStoreError> {
        let scripted = self
            .fail_reads_of
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match scripted {
            Some((failing, error)) if &failing == reference => Err(error),
            _ => self.inner.read(reference, interaction),
        }
    }

    fn write(
        &self,
        reference: &SecretRef,
        value: &SecretValue,
        mode: WriteMode,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        let mut writes_ok = self
            .writes_ok
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match writes_ok.as_mut() {
            Some(0) => return Err(SecretStoreError::Backend { code: -1 }),
            Some(left) => *left -= 1,
            None => {}
        }
        drop(writes_ok);
        self.inner.write(reference, value, mode, interaction)
    }

    fn delete(
        &self,
        reference: &SecretRef,
        interaction: Interaction,
    ) -> Result<(), SecretStoreError> {
        self.inner.delete(reference, interaction)
    }
}
