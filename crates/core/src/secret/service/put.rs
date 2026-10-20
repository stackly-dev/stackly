//! `put`: the one use case that stores a value behind a reference (spec
//! 0011, *Use case isi atau ganti nilai*, AC-5, AC-6).

use stackly_domain::{
    ResourceRevision, SecretFieldName, SecretOrigin, SecretOwner, SecretPurpose, SecretRef,
    SecretReferenceId, SecretReferenceRecord, SecretReferenceView, SecretStatus, WriteBatch,
    WriteOp,
};
use stackly_platform::{SCRUB_MIN_LEN, SecretPresence, SecretValue, WriteMode};

use super::audit::SET;
use super::{SecretCaller, SecretChange, SecretService};
use crate::secret::error::{ConflictReason, SecretError};
use crate::secret::generate::{GENERATED_LEN, generate_password};
use crate::secret::redact::{char_count, has_control_character};
use crate::secret::rules::{Bounds, EffectKind, InvalidValueReason, SecretEffect, SecretFieldRule};

/// Where `put` stores the value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PutTarget {
    /// A reference that is registered, or that `secret.set` registers for
    /// the user: `secret.set`, and filling a reference after a needs
    /// secret error.
    Existing(SecretRef),
    /// A new reference of a new resource: create instance, env up, and
    /// restore. `put` registers the row; the caller's operation writes the
    /// use with `ReplaceSecretUses` in the batch that registers the
    /// resource.
    New {
        /// The reference, `local:instance/<id>/<purpose>` for an instance.
        reference: SecretRef,
        /// Its owner.
        owner: SecretOwner,
        /// The purpose of its field.
        purpose: SecretPurpose,
        /// The secret field it is made for, whose rules the value must
        /// keep: set for create instance and restore, empty for env up.
        /// With a field, a bind reads the bound value once to check it.
        field: Option<FieldContext>,
    },
}

/// The secret field a new reference is made for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FieldContext {
    /// The field's name.
    pub name: SecretFieldName,
    /// Its rules from the provider manifest.
    pub rule: SecretFieldRule,
}

impl FieldContext {
    /// Names a field and its rules.
    #[must_use]
    pub const fn new(name: SecretFieldName, rule: SecretFieldRule) -> Self {
        Self { name, rule }
    }
}

/// Where the value comes from.
#[derive(Debug)]
#[non_exhaustive]
pub enum SecretInput {
    /// A value a client sent through its secure input.
    Value(SecretValue),
    /// A password the daemon generates: 32 characters of `[A-Za-z0-9]`,
    /// clamped to the bounds of the field.
    Generate,
    /// The value behind another registered reference, read from the store
    /// and never returned.
    CopyFrom(SecretRef),
    /// No value: the new resource uses this registered reference. Valid
    /// only with [`PutTarget::New`], whose reference, owner, and purpose it
    /// replaces; nothing is written, and the caller records the use. With
    /// the target's field, the bound value is read once and checked against
    /// the field's rules (AC-23); without one, the store is not touched.
    Bind(SecretRef),
}

/// One request to [`SecretService::put`].
#[derive(Debug)]
#[non_exhaustive]
pub struct PutRequest {
    /// Where the value goes.
    pub target: PutTarget,
    /// Where it comes from.
    pub input: SecretInput,
    /// `create` fails on an existing value, `replace` on a missing one,
    /// and `upsert` takes either.
    pub mode: WriteMode,
    /// The revision the client read, if it names one.
    pub expected_revision: Option<ResourceRevision>,
    /// The client confirmed that a `stored_only` replace changes only the
    /// stored copy.
    pub confirm_stored_only: bool,
}

impl PutRequest {
    /// A request without an expected revision or a confirmation.
    #[must_use]
    pub const fn new(target: PutTarget, input: SecretInput, mode: WriteMode) -> Self {
        Self {
            target,
            input,
            mode,
            expected_revision: None,
            confirm_stored_only: false,
        }
    }

    /// The reference the request locks: the bound one for a bind.
    fn reference(&self) -> &SecretRef {
        match (&self.input, &self.target) {
            (SecretInput::Bind(bound), _) => bound,
            (_, PutTarget::Existing(reference) | PutTarget::New { reference, .. }) => reference,
        }
    }
}

/// A warning a stored value comes with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PutWarning {
    /// The value is shorter than 8 bytes, so redaction never matches it in
    /// text (`short_value_not_scrubbed`).
    ShortValueNotScrubbed,
}

impl PutWarning {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::ShortValueNotScrubbed => "short_value_not_scrubbed",
        }
    }
}

/// What a successful `put` returns: never the value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PutOutcome {
    /// The reference row after the put, with its uses.
    pub reference: SecretReferenceView,
    /// The warnings.
    pub warnings: Vec<PutWarning>,
    /// The effect on every use.
    pub effects: Vec<SecretEffect>,
}

/// The registry row a put writes.
enum Row {
    /// A registered row, and its uses.
    Registered(SecretReferenceView),
    /// A row to register.
    New(SecretReferenceRecord),
}

/// Whether a failure happened in step 1 (parse and reserved segments),
/// which writes no audit row.
struct Failed {
    error: SecretError,
    audited: bool,
}

impl From<SecretError> for Failed {
    fn from(error: SecretError) -> Self {
        Self {
            error,
            audited: true,
        }
    }
}

impl SecretService {
    /// Stores a value behind a reference: the one use case for
    /// `secret.set`, the GUI and CLI, create instance, env up, and restore
    /// (AC-5).
    ///
    /// In order: the registry and `expected_revision` (a conflict stops
    /// before the store is touched); the value, learned by the redactor at
    /// once and checked against the rules of every field that uses the
    /// reference, length first and then control characters (a `Value`
    /// that breaks them never reaches the store); a probe of the store and
    /// the mode against it, reading the old value for a replace; the
    /// `stored_only` confirmation; the store write; and one registry
    /// commit that writes the row (status `present`, revision up by
    /// exactly 1, or 1 for a new row) with its audit row. When that commit
    /// fails, the old value goes back into the store (or the new item is
    /// deleted). A bind writes nothing; with a field, it reads the bound
    /// value once, learns it, and checks it. Every failure after step 1
    /// writes a `failed` audit row of its own. The reference is held for
    /// the whole call, and the change is reported after it is released.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::NotFound`] for an unregistered instance
    /// reference, a registered row missing where `replace` needs it, or an
    /// unregistered copy or bind source; [`SecretError::InvalidReference`]
    /// for an unregistered reference under `stackly`;
    /// [`SecretError::RevisionMismatch`]; [`SecretError::Conflict`] for
    /// `create` on an existing value or a new resource naming a registered
    /// reference; [`SecretError::InvalidValue`] with its reason;
    /// [`SecretError::ConfirmationRequired`]; [`SecretError::Store`] with
    /// the reference whose store call failed (a registered source the
    /// store holds no value for is a `NotFound` store error, which
    /// [`classify_create_failure`](crate::secret::classify_create_failure)
    /// tells apart from an unregistered one);
    /// [`SecretError::WriteBackFailed`]; and [`SecretError::Registry`].
    pub fn put(
        &self,
        request: PutRequest,
        caller: &SecretCaller,
    ) -> Result<PutOutcome, SecretError> {
        let reference = request.reference().clone();
        let guard = self.locks.lock(&reference);
        let result = self.put_held(request, caller);
        drop(guard);
        match result {
            Ok((outcome, change)) => {
                if let Some(change) = change {
                    self.report(&change);
                }
                Ok(outcome)
            }
            Err(failed) => {
                if failed.audited {
                    self.audit_failure(SET, caller, &reference, None, &failed.error);
                }
                Err(failed.error)
            }
        }
    }

    fn put_held(
        &self,
        request: PutRequest,
        caller: &SecretCaller,
    ) -> Result<(PutOutcome, Option<SecretChange>), Failed> {
        let PutRequest {
            target,
            input,
            mode,
            expected_revision,
            confirm_stored_only,
        } = request;
        if let SecretInput::Bind(bound) = input {
            return self
                .bind(&target, &bound, expected_revision, caller)
                .map(|outcome| (outcome, None));
        }
        // Steps 1 and 2: the registry row and the revision.
        let (row, bounds, effects) = self.registry_row(target, expected_revision)?;
        let reference = match &row {
            Row::Registered(view) => view.record.reference.clone(),
            Row::New(record) => record.reference.clone(),
        };
        // Step 3: the value.
        let (value, origin) = self.prepare(input, &bounds, caller)?;
        // Step 4: the store and the mode.
        let interaction = caller.interaction;
        let at_target = |error| SecretError::Store {
            reference: reference.clone(),
            error,
        };
        let presence = self.store().probe(&reference).map_err(at_target)?;
        let old = match (mode, presence) {
            (WriteMode::Create, SecretPresence::Present) => {
                return Err(SecretError::Conflict {
                    reference,
                    reason: ConflictReason::AlreadyExists,
                }
                .into());
            }
            (WriteMode::Replace, SecretPresence::Missing) => {
                return Err(SecretError::NotFound { reference }.into());
            }
            (_, SecretPresence::Present) => {
                let old = self
                    .store()
                    .read(&reference, interaction)
                    .map_err(at_target)?;
                self.deps.redactor.learn(&old);
                Some(old)
            }
            (_, SecretPresence::Missing) => None,
        };
        // Step 5: a value that only the stored copy would take.
        if old.is_some()
            && !confirm_stored_only
            && effects
                .iter()
                .any(|effect| effect.effect == EffectKind::StoredOnly)
        {
            return Err(SecretError::ConfirmationRequired { reference, effects }.into());
        }
        // Step 6: the store.
        let write_mode = if old.is_some() {
            WriteMode::Replace
        } else {
            WriteMode::Create
        };
        self.store()
            .write(&reference, &value, write_mode, interaction)
            .map_err(at_target)?;
        // Step 7: one registry commit for the row and its audit row.
        let now = self.now();
        let store_key = self.store_key(&reference)?;
        let (record, uses, op) = match row {
            Row::Registered(view) => {
                let mut record = view.record.clone();
                record.revision = view.record.revision.next();
                record.platform_store_key = store_key;
                record.value_origin = Some(origin);
                record.status = SecretStatus::Present;
                record.status_observed_at = Some(now);
                record.value_updated_at = Some(now);
                record.updated_at = now;
                let op = WriteOp::UpdateSecretReference {
                    record: record.clone(),
                    expected: view.record.revision,
                };
                (record, view.used_by, op)
            }
            Row::New(mut record) => {
                record.platform_store_key = store_key;
                record.value_origin = Some(origin);
                record.status = SecretStatus::Present;
                record.status_observed_at = Some(now);
                record.value_updated_at = Some(now);
                record.created_at = now;
                record.updated_at = now;
                let op = WriteOp::InsertSecretReference(record.clone());
                (record, Vec::new(), op)
            }
        };
        let audit = self.audit_row(SET, caller, &reference, Some(record.id), None, None);
        let committed = self
            .registry()
            .commit(WriteBatch::new(vec![op, WriteOp::InsertAuditEvent(audit)]));
        if let Err(failed) = committed {
            let undone = match &old {
                Some(old) => self
                    .store()
                    .write(&reference, old, WriteMode::Replace, interaction),
                None => self.store().delete(&reference, interaction),
            };
            let error = match undone {
                Ok(()) => SecretError::from(failed.error),
                Err(store) => {
                    tracing::error!(
                        %reference,
                        %store,
                        "the secret store change was not undone after the registry failed"
                    );
                    SecretError::WriteBackFailed {
                        registry: failed.error,
                    }
                }
            };
            return Err(error.into());
        }
        // Step 8: the summary, warnings, and effects.
        let mut warnings = Vec::new();
        if value.len() < SCRUB_MIN_LEN {
            warnings.push(PutWarning::ShortValueNotScrubbed);
        }
        let view = SecretReferenceView::new(record, uses);
        let change = Self::change_of(&view);
        Ok((
            PutOutcome {
                reference: view,
                warnings,
                effects,
            },
            Some(change),
        ))
    }

    /// Steps 1 and 2: the row to write, the bounds of the fields that use
    /// the reference, and the effect on each use.
    fn registry_row(
        &self,
        target: PutTarget,
        expected: Option<ResourceRevision>,
    ) -> Result<(Row, Bounds, Vec<SecretEffect>), Failed> {
        let reference = match &target {
            PutTarget::Existing(reference) | PutTarget::New { reference, .. } => reference.clone(),
        };
        let stored = Self::read(self.registry().secret_reference(&reference))?;
        let row = match (target, stored) {
            (PutTarget::Existing(_), Some(view)) => Row::Registered(view),
            (PutTarget::Existing(_), None) if reference.is_instance_reference() => {
                // Only the daemon registers an instance reference.
                return Err(Failed {
                    error: SecretError::NotFound { reference },
                    audited: false,
                });
            }
            (PutTarget::Existing(_), None) if reference.is_reserved() => {
                return Err(Failed {
                    error: SecretError::InvalidReference {
                        reference,
                        reason: "the first segment stackly is reserved",
                    },
                    audited: false,
                });
            }
            (PutTarget::Existing(_), None) => Row::New(self.new_record(
                reference.clone(),
                SecretOwner::User,
                SecretPurpose::user(),
            )?),
            (PutTarget::New { .. }, Some(_)) => {
                // A new resource never takes a registered reference on its
                // own; sharing goes through bind.
                return Err(SecretError::Conflict {
                    reference,
                    reason: ConflictReason::AlreadyExists,
                }
                .into());
            }
            (
                PutTarget::New {
                    owner,
                    purpose,
                    field,
                    ..
                },
                None,
            ) => {
                if expected.is_some() {
                    return Err(SecretError::NotFound { reference }.into());
                }
                let bounds = field.map_or_else(Bounds::default, |field| {
                    Bounds::single(field.name, field.rule)
                });
                let record = self.new_record(reference, owner, purpose)?;
                return Ok((Row::New(record), bounds, Vec::new()));
            }
        };
        match (&row, expected) {
            (Row::Registered(view), Some(expected)) if view.record.revision != expected => {
                return Err(SecretError::RevisionMismatch {
                    reference,
                    expected,
                    actual: view.record.revision,
                }
                .into());
            }
            (Row::New(_), Some(_)) => {
                return Err(SecretError::NotFound { reference }.into());
            }
            _ => {}
        }
        let mut bounds = Bounds::default();
        let mut effects = Vec::new();
        if let Row::Registered(view) = &row {
            for used in &view.used_by {
                let rules = self.use_rules(used.user, &used.field)?;
                if let Some(rule) = rules.rule {
                    bounds.add(used.field.clone(), rule);
                }
                effects.push(SecretEffect::new(
                    used.user,
                    used.field.clone(),
                    rules.effect,
                ));
            }
        }
        Ok((row, bounds, effects))
    }

    /// A row to register for `reference`, at revision 1 and with no value
    /// yet; step 7 fills the value columns.
    fn new_record(
        &self,
        reference: SecretRef,
        owner: SecretOwner,
        purpose: SecretPurpose,
    ) -> Result<SecretReferenceRecord, SecretError> {
        let store_key = self.store_key(&reference)?;
        Ok(SecretReferenceRecord::new(
            SecretReferenceId::generate(),
            reference,
            store_key,
            owner,
            purpose,
            self.now(),
        ))
    }

    /// Step 3: the value and where it came from, learned at once and
    /// checked against the rules of every field that uses the reference.
    /// A generated value always keeps them, once its length is clamped.
    fn prepare(
        &self,
        input: SecretInput,
        bounds: &Bounds,
        caller: &SecretCaller,
    ) -> Result<(SecretValue, SecretOrigin), SecretError> {
        let (value, origin) = match input {
            SecretInput::Generate => {
                let length = bounds
                    .clamp(u64::try_from(GENERATED_LEN).unwrap_or(u64::MAX))
                    .map_err(|broken| invalid(broken, InvalidValueReason::Length))?;
                let length = usize::try_from(length).map_err(|_| SecretError::Internal {
                    reason: "a generated length does not fit this platform".to_owned(),
                })?;
                let value = generate_password(length).map_err(|err| SecretError::Internal {
                    reason: err.to_string(),
                })?;
                (value, SecretOrigin::Generated)
            }
            SecretInput::Value(value) => (value, SecretOrigin::Entered),
            SecretInput::CopyFrom(source) => {
                let registered = Self::read(self.registry().secret_reference(&source))?;
                if registered.is_none() {
                    return Err(SecretError::NotFound { reference: source });
                }
                let value = self
                    .store()
                    .read(&source, caller.interaction)
                    .map_err(|error| SecretError::Store {
                        reference: source.clone(),
                        error,
                    })?;
                (value, SecretOrigin::Copied)
            }
            SecretInput::Bind(_) => {
                return Err(SecretError::Internal {
                    reason: "bind stores no value".to_owned(),
                });
            }
        };
        self.deps.redactor.learn(&value);
        if origin != SecretOrigin::Generated {
            check(bounds, &value)?;
        }
        Ok((value, origin))
    }

    /// A bind: the new resource uses a registered reference, and nothing
    /// is written. With the target's field, the bound value is read once
    /// with the caller's interaction, learned, and checked against the
    /// field's rules; it is an internal read, so no reveal is audited.
    fn bind(
        &self,
        target: &PutTarget,
        bound: &SecretRef,
        expected: Option<ResourceRevision>,
        caller: &SecretCaller,
    ) -> Result<PutOutcome, Failed> {
        let field = match target {
            PutTarget::New { field, .. } => field.as_ref(),
            PutTarget::Existing(_) => {
                return Err(SecretError::Internal {
                    reason: "bind is only for a new resource".to_owned(),
                }
                .into());
            }
        };
        let view = Self::read(self.registry().secret_reference(bound))?.ok_or_else(|| {
            SecretError::NotFound {
                reference: bound.clone(),
            }
        })?;
        if let Some(expected) = expected
            && view.record.revision != expected
        {
            return Err(SecretError::RevisionMismatch {
                reference: bound.clone(),
                expected,
                actual: view.record.revision,
            }
            .into());
        }
        if let Some(field) = field {
            let value = self
                .store()
                .read(bound, caller.interaction)
                .map_err(|error| SecretError::Store {
                    reference: bound.clone(),
                    error,
                })?;
            self.deps.redactor.learn(&value);
            check(&Bounds::single(field.name.clone(), field.rule), &value)?;
        }
        Ok(PutOutcome {
            reference: view,
            warnings: Vec::new(),
            effects: Vec::new(),
        })
    }
}

/// Checks a value against the rules of every field in `bounds`, in order,
/// each by its length first and then its control characters (AC-22).
fn check(bounds: &Bounds, value: &SecretValue) -> Result<(), SecretError> {
    let chars = u64::try_from(char_count(value)).unwrap_or(u64::MAX);
    match bounds.broken_by(chars, has_control_character(value)) {
        Some((broken, reason)) => Err(invalid(broken, reason)),
        None => Ok(()),
    }
}

/// The error for a value that breaks the rules of `field` for `reason`. It
/// names the field and its bounds, never the value.
fn invalid(
    (field, rule): &(SecretFieldName, SecretFieldRule),
    reason: InvalidValueReason,
) -> SecretError {
    SecretError::InvalidValue {
        field: field.clone(),
        min_length: rule.min_length,
        max_length: rule.max_length,
        reason,
    }
}
