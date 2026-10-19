//! Reading every value an instance needs before it starts, and learning
//! the values of the instances that run already (spec 0011, *Needs
//! secret*, AC-11).

use std::collections::BTreeMap;

use stackly_domain::{
    InstanceId, InstanceLifecycle, NeedsSecretItem, NeedsSecretReason, RegistryError, SecretRef,
    SecretStatus, SecretUser, WriteBatch,
};
use stackly_platform::{Interaction, SecretStoreError, SecretValue};

use super::{SecretCaller, SecretChange, SecretService};
use crate::provider::SettingName;
use crate::secret::delivery::counts_as_running;
use crate::secret::error::{NeedsSecret, SecretError};
use crate::secret::redact::copy_value;
use crate::secret::resolved::ResolvedSecrets;

/// How many values `learn_running` learned, and how many reads failed, by
/// reason.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct LearnReport {
    /// The values learned.
    pub learned: usize,
    /// The reads that failed, by reason code (`missing`, `store_locked`,
    /// `access_denied`, `store_busy`, `store_unsupported`, or `backend`).
    pub failed: BTreeMap<&'static str, usize>,
}

/// What one read of the store gave.
enum Read {
    Value(SecretValue),
    Needs(NeedsSecretReason),
    Backend(SecretStoreError),
}

fn classify(result: Result<SecretValue, SecretStoreError>) -> Read {
    match result {
        Ok(value) => Read::Value(value),
        Err(SecretStoreError::NotFound) => Read::Needs(NeedsSecretReason::Missing),
        Err(SecretStoreError::Locked) => Read::Needs(NeedsSecretReason::StoreLocked),
        Err(SecretStoreError::AccessDenied { .. }) => Read::Needs(NeedsSecretReason::AccessDenied),
        Err(SecretStoreError::Busy | SecretStoreError::OutcomeUnknown) => {
            Read::Needs(NeedsSecretReason::StoreBusy)
        }
        Err(SecretStoreError::NotSupported { .. }) => {
            Read::Needs(NeedsSecretReason::StoreUnsupported)
        }
        Err(other) => Read::Backend(other),
    }
}

impl SecretService {
    /// Reads every value an instance needs, from the uses of its active
    /// config revision, always from the store and never from the cache
    /// (AC-11).
    ///
    /// References are read in text order with the caller's interaction.
    /// After the user denies one dialog, the rest are not read and count as
    /// `access_denied`, so no new dialog appears. A missing value records
    /// `missing` in the cache. Nothing ever stands in for a value that
    /// cannot be read.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::NeedsSecret`] with every reference that
    /// could not be read, why, and whether a generated value would work,
    /// in reference order; [`SecretError::Store`] for a backend failure,
    /// which wins over needs secret; and [`SecretError::Registry`].
    pub fn resolve_for_start(
        &self,
        instance: InstanceId,
        caller: &SecretCaller,
    ) -> Result<ResolvedSecrets, SecretError> {
        let uses = Self::read(
            self.registry()
                .secret_uses_by(SecretUser::Instance(instance)),
        )?;
        let mut fields: BTreeMap<SecretRef, Vec<SettingName>> = BTreeMap::new();
        for used in uses {
            let name = used.field.as_str().parse::<SettingName>().map_err(|err| {
                SecretError::Registry(RegistryError::Corrupt {
                    table: "secret_reference_uses",
                    column: "field",
                    detail: format!("an instance field is not a setting name: {err}"),
                })
            })?;
            fields.entry(used.reference).or_default().push(name);
        }
        let mut values = BTreeMap::new();
        let mut needs = Vec::new();
        let mut denied_by_user = false;
        for (reference, names) in fields {
            let read = if denied_by_user {
                Read::Needs(NeedsSecretReason::AccessDenied)
            } else {
                let guard = self.locks.lock(&reference);
                let result = self.store().read(&reference, caller.interaction);
                denied_by_user = matches!(
                    result,
                    Err(SecretStoreError::AccessDenied { by_user: true })
                );
                let read = classify(result);
                let change = if matches!(read, Read::Needs(NeedsSecretReason::Missing)) {
                    self.record_missing(&reference)
                } else {
                    None
                };
                drop(guard);
                if let Some(change) = change {
                    self.report(&change);
                }
                read
            };
            match read {
                Read::Value(value) => {
                    self.deps.redactor.learn(&value);
                    let mut names = names.into_iter();
                    let first = names.next();
                    for name in names {
                        values.insert(name, copy_value(&value));
                    }
                    if let Some(first) = first {
                        values.insert(first, value);
                    }
                }
                Read::Needs(reason) => {
                    let allowed = self.generate_allowed(&reference)?;
                    needs.push(NeedsSecretItem::new(reference, reason, allowed));
                }
                Read::Backend(error) => return Err(SecretError::Store(error)),
            }
        }
        if needs.is_empty() {
            Ok(ResolvedSecrets::new(values))
        } else {
            Err(SecretError::NeedsSecret {
                needs: NeedsSecret { references: needs },
            })
        }
    }

    /// Learns the value of every reference that the running instances use,
    /// without a dialog, so the logs of adopted processes stay redacted
    /// before any new start (spec 0011, *Redaction*). An instance counts as
    /// running in `starting`, `running`, `recovering`, `stopping`, or
    /// `unknown`. A read that fails is counted by reason and blocks
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::Registry`] when the uses cannot be read.
    pub fn learn_running(
        &self,
        instances: impl IntoIterator<Item = (InstanceId, InstanceLifecycle)>,
    ) -> Result<LearnReport, SecretError> {
        let mut references = Vec::new();
        for (instance, lifecycle) in instances {
            if !counts_as_running(lifecycle) {
                continue;
            }
            let uses = Self::read(
                self.registry()
                    .secret_uses_by(SecretUser::Instance(instance)),
            )?;
            references.extend(uses.into_iter().map(|used| used.reference));
        }
        references.sort();
        references.dedup();
        let mut report = LearnReport::default();
        for reference in references {
            let reason = match classify(self.store().read(&reference, Interaction::Never)) {
                Read::Value(value) => {
                    self.deps.redactor.learn(&value);
                    report.learned += 1;
                    continue;
                }
                Read::Needs(reason) => reason.as_code(),
                Read::Backend(_) => "backend",
            };
            *report.failed.entry(reason).or_default() += 1;
        }
        tracing::info!(
            learned = report.learned,
            failed = ?report.failed,
            "learned the secret values of the running instances"
        );
        Ok(report)
    }

    /// Whether a generated value can work for every use of a reference:
    /// not when one use is a field fixed after the data is initialized, on
    /// an instance whose data is.
    fn generate_allowed(&self, reference: &SecretRef) -> Result<bool, SecretError> {
        let Some(view) = Self::read(self.registry().secret_reference(reference))? else {
            return Ok(true);
        };
        for used in &view.used_by {
            if !self.use_rules(used.user, &used.field)?.generate_allowed() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Records that the store holds no value for a reference, which the
    /// caller holds, and returns the change to report once it is released.
    /// A row that is gone, or a commit that fails, is logged and changes
    /// nothing else.
    fn record_missing(&self, reference: &SecretRef) -> Option<SecretChange> {
        let observed = (|| {
            let view = Self::read(self.registry().secret_reference(reference))?;
            let Some(view) = view else {
                return Ok(None);
            };
            let key = self.store_key(reference)?;
            let at = self.now();
            let op = Self::observation(&view, SecretStatus::Missing, key.clone(), at);
            self.registry()
                .commit(WriteBatch::new(vec![op]))
                .map_err(|failed| SecretError::from(failed.error))?;
            Ok::<_, SecretError>(Self::observed(view, SecretStatus::Missing, key, at).1)
        })();
        observed.unwrap_or_else(|error| {
            tracing::warn!(%reference, %error, "a missing secret value was not recorded");
            None
        })
    }
}
