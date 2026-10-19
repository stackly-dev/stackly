//! The secret values of one action.

use std::collections::BTreeMap;
use std::fmt;

use stackly_platform::SecretValue;

use crate::provider::SettingName;

/// The secret values of one action, by secret field, as
/// `SecretService::resolve_for_start` of spec 0011 returns them.
///
/// Only the functions of [`delivery`](super::delivery) and
/// [`RenderedContent::materialize`](crate::provider::RenderedContent::materialize)
/// read the values. It has no `Default` and no `Clone`, every value is
/// wiped from memory when it drops, and `Debug` shows only the field names.
pub struct ResolvedSecrets(BTreeMap<SettingName, SecretValue>);

impl ResolvedSecrets {
    /// Holds the given value for each field. The secret service is the
    /// only caller outside tests.
    pub(crate) fn new(values: BTreeMap<SettingName, SecretValue>) -> Self {
        Self(values)
    }

    /// Holds the given value for each field. Only dev dependencies and
    /// integration tests turn on the `test-support` feature that builds one
    /// this way; the daemon gets one from the secret store.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn for_tests(values: impl IntoIterator<Item = (SettingName, SecretValue)>) -> Self {
        Self::new(values.into_iter().collect())
    }

    /// Returns the value of a secret field.
    #[must_use]
    pub fn get(&self, field: &SettingName) -> Option<&SecretValue> {
        self.0.get(field)
    }

    /// Returns the secret fields, in name order.
    pub fn fields(&self) -> impl Iterator<Item = &SettingName> {
        self.0.keys()
    }

    /// Returns each field with its value, in field name order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&SettingName, &SecretValue)> {
        self.0.iter()
    }
}

impl fmt::Debug for ResolvedSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(self.0.keys().map(SettingName::as_str))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_secrets_show_only_their_fields() {
        let field: SettingName = "authToken".parse().unwrap();
        let value = SecretValue::new("hunter2-but-longer".to_owned()).unwrap();
        let secrets = ResolvedSecrets::new(BTreeMap::from([(field.clone(), value)]));

        assert!(secrets.get(&field).is_some());
        assert!(secrets.get(&"other".parse().unwrap()).is_none());
        assert_eq!(secrets.fields().collect::<Vec<_>>(), [&field]);
        assert_eq!(format!("{secrets:?}"), "{\"authToken\"}");
    }
}
