//! Provider capabilities: what a provider revision supports for its
//! instances.

use std::collections::BTreeSet;

use crate::code::code_enum;
use crate::error::{InvariantDetail, InvariantError};

code_enum! {
    /// One thing a provider can do for its instances. Feature 9 maps the
    /// provider manifest onto these.
    ///
    /// The name differs from `PlatformCapability` in `stackly-platform`,
    /// which is about the OS adapter.
    #[non_exhaustive]
    pub enum Capability: Capability {
        /// Reload the configuration without a restart.
        Reload => "reload",
        /// Back up while the instance runs.
        OnlineBackup => "online_backup",
        /// Back up while the instance is stopped.
        ColdBackup => "cold_backup",
        /// Back up as a logical dump.
        LogicalBackup => "logical_backup",
        /// Serve TLS.
        Tls => "tls",
        /// Expose metrics.
        Metrics => "metrics",
        /// Run more than one instance side by side.
        MultiInstance => "multi_instance",
        /// Keep data across restarts in the instance's data folder.
        Persistence => "persistence",
    }
}

/// The capabilities of one provider revision.
///
/// ```
/// use stackly_domain::{Capability, CapabilitySet, Rule};
///
/// let caps: CapabilitySet = [Capability::Reload].into_iter().collect();
/// assert!(caps.require(Capability::Reload).is_ok());
/// let err = caps.require(Capability::Tls).unwrap_err();
/// assert_eq!(err.rule(), Some(Rule::Br016));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    /// Creates the set from a list of capabilities, such as the ones a
    /// provider manifest declares. Duplicates collapse into one.
    ///
    /// ```
    /// use stackly_domain::{Capability, CapabilitySet};
    ///
    /// let caps = CapabilitySet::new([Capability::Persistence, Capability::MultiInstance]);
    /// assert!(caps.contains(Capability::MultiInstance));
    /// ```
    #[must_use]
    pub fn new(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self(capabilities.into_iter().collect())
    }

    /// Returns whether the set holds `capability`.
    #[must_use]
    pub fn contains(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    /// Checks that the provider supports `capability` before anything is
    /// attempted.
    ///
    /// # Errors
    ///
    /// Returns an [`InvariantError`] for BR-016 with
    /// [`InvariantDetail::Unsupported`] when the set lacks the capability.
    pub fn require(&self, capability: Capability) -> Result<(), InvariantError> {
        if self.contains(capability) {
            Ok(())
        } else {
            Err(InvariantDetail::Unsupported { capability }.into())
        }
    }

    /// Returns every capability in code order.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        Self::new(iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_round_trip_through_their_codes() {
        for capability in Capability::ALL {
            assert_eq!(
                capability.as_code().parse::<Capability>().unwrap(),
                *capability
            );
        }
        assert!("Reload".parse::<Capability>().is_err());
    }

    #[test]
    fn feature_9_capabilities_have_snake_case_codes() {
        assert_eq!(Capability::MultiInstance.as_code(), "multi_instance");
        assert_eq!(Capability::Persistence.as_code(), "persistence");
        assert_eq!(
            "multi_instance".parse::<Capability>().unwrap(),
            Capability::MultiInstance
        );
    }

    #[test]
    fn a_set_built_from_a_list_matches_one_collected_from_it() {
        let list = [
            Capability::Persistence,
            Capability::Reload,
            Capability::Persistence,
        ];

        let set = CapabilitySet::new(list);

        assert_eq!(set, list.into_iter().collect());
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [Capability::Reload, Capability::Persistence]
        );
    }

    #[test]
    fn a_set_holds_each_capability_once_in_code_order() {
        let set: CapabilitySet = [Capability::Tls, Capability::Reload, Capability::Tls]
            .into_iter()
            .collect();

        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [Capability::Reload, Capability::Tls]
        );
        assert!(CapabilitySet::default().iter().next().is_none());
    }
}
