//! What a secret field of a provider manifest says about its value: its
//! bounds, and what a new value changes for a resource that uses it (spec
//! 0011, *Use case isi atau ganti nilai*, and spec 0009).

use std::fmt;

use stackly_domain::{Instance, SecretFieldName, SecretUser};

use crate::provider::{ApplyClass, FieldKind, ScalarKind, SettingField};

/// The rules of one secret field from the settings schema of a provider
/// manifest (spec 0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretFieldRule {
    /// The fewest characters of a value.
    pub min_length: u64,
    /// The most characters of a value.
    pub max_length: Option<u64>,
    /// When a change reaches the process.
    pub apply: Option<ApplyClass>,
    /// The value holds no control character (`x-stackly.printableOnly`).
    pub printable_only: bool,
}

impl SecretFieldRule {
    /// The rules of a field with the given bounds and apply class, which
    /// takes control characters.
    #[must_use]
    pub const fn new(min_length: u64, max_length: Option<u64>, apply: Option<ApplyClass>) -> Self {
        Self {
            min_length,
            max_length,
            apply,
            printable_only: false,
        }
    }

    /// Returns the rules with `printable_only` set: whether the value must
    /// hold no control character.
    #[must_use]
    pub const fn with_printable_only(mut self, printable_only: bool) -> Self {
        self.printable_only = printable_only;
        self
    }

    /// Reads the rules of a secret field of a settings schema, or `None`
    /// when the field is not a secret text field. `printable_only` follows
    /// `x-stackly.printableOnly`.
    #[must_use]
    pub fn from_setting(field: &SettingField) -> Option<Self> {
        if !field.is_secret() {
            return None;
        }
        match field.kind() {
            FieldKind::Scalar(ScalarKind::Text(rules)) => Some(
                Self::new(rules.min_length, rules.max_length, field.apply())
                    .with_printable_only(field.is_printable_only()),
            ),
            _ => None,
        }
    }

    /// Checks a value of `chars` characters, `has_control` when it holds a
    /// control character: its length first, then, for a `printable_only`
    /// field, its control characters (AC-22).
    ///
    /// # Errors
    ///
    /// Returns [`InvalidValueReason::Length`] when `chars` is outside
    /// `min_length` and `max_length`, and
    /// [`InvalidValueReason::ControlCharacters`] when the length fits but
    /// the field is `printable_only` and the value holds a control
    /// character.
    pub const fn check(&self, chars: u64, has_control: bool) -> Result<(), InvalidValueReason> {
        let too_long = match self.max_length {
            Some(max) => chars > max,
            None => false,
        };
        if chars < self.min_length || too_long {
            Err(InvalidValueReason::Length)
        } else if self.printable_only && has_control {
            Err(InvalidValueReason::ControlCharacters)
        } else {
            Ok(())
        }
    }
}

/// Why a value breaks the rules of a secret field, the `reason` detail of
/// `secret.invalid_value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InvalidValueReason {
    /// Its character count is outside `minLength` and `maxLength`.
    Length,
    /// The field is `printableOnly`, and the value holds a control
    /// character.
    ControlCharacters,
}

impl InvalidValueReason {
    /// Returns the stable code, `length` or `control_characters`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::ControlCharacters => "control_characters",
        }
    }
}

impl fmt::Display for InvalidValueReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Finds the rules of the secret fields an instance uses, from the
/// provider manifest of its provider revision (spec 0009). The daemon
/// implements it over the provider packages it loaded.
pub trait SecretFieldRules: Send + Sync {
    /// Returns the rules of `field` of `instance`, or `None` when its
    /// provider package or the field is not known.
    fn rules(&self, instance: &Instance, field: &SecretFieldName) -> Option<SecretFieldRule>;
}

/// What a new value changes for one use of a reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EffectKind {
    /// Only the stored copy changes: the field is fixed once the data is
    /// initialized, and it is, so the value inside the data stays.
    StoredOnly,
    /// The next reload uses it.
    NextReload,
    /// The next start uses it.
    NextStart,
}

impl EffectKind {
    /// Returns the stable code, for example `stored_only`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::StoredOnly => "stored_only",
            Self::NextReload => "next_reload",
            Self::NextStart => "next_start",
        }
    }
}

impl fmt::Display for EffectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// The effect of a new value on one use.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SecretEffect {
    /// The resource.
    pub user: SecretUser,
    /// Its field.
    pub field: SecretFieldName,
    /// What changes.
    pub effect: EffectKind,
}

impl SecretEffect {
    /// Describes the effect on one use.
    #[must_use]
    pub const fn new(user: SecretUser, field: SecretFieldName, effect: EffectKind) -> Self {
        Self {
            user,
            field,
            effect,
        }
    }
}

/// What a use of a reference allows, from the rules of its field and the
/// data of its instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UseRules {
    /// The bounds of the field, when they are known.
    pub(crate) rule: Option<SecretFieldRule>,
    /// What a new value changes.
    pub(crate) effect: EffectKind,
}

impl UseRules {
    /// The rules of an environment's use, which has no field rules.
    pub(crate) const ENVIRONMENT: Self = Self {
        rule: None,
        effect: EffectKind::NextStart,
    };

    /// The rules of a use by `instance`. A field that is fixed once the
    /// data is initialized, of an instance whose data is, changes only the
    /// stored copy. So does a field whose rules are unknown on such an
    /// instance, since nothing proves the value inside the data follows.
    pub(crate) fn of_instance(instance: &Instance, rule: Option<SecretFieldRule>) -> Self {
        let initialized = instance.data_initialized_at().is_some();
        let effect = match rule.and_then(|rule| rule.apply) {
            Some(ApplyClass::ImmutableAfterInit) if initialized => EffectKind::StoredOnly,
            None if initialized && rule.is_none() => EffectKind::StoredOnly,
            Some(ApplyClass::Reload) => EffectKind::NextReload,
            _ => EffectKind::NextStart,
        };
        Self { rule, effect }
    }

    /// Whether a generated value can work for this use: not when only the
    /// stored copy would change.
    pub(crate) fn generate_allowed(self) -> bool {
        self.effect != EffectKind::StoredOnly
    }
}

/// The bounds every use of a reference shares, and the field each bound
/// comes from, so an error can name it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Bounds {
    fields: Vec<(SecretFieldName, SecretFieldRule)>,
}

impl Bounds {
    /// Bounds with one field.
    pub(crate) fn single(field: SecretFieldName, rule: SecretFieldRule) -> Self {
        Self {
            fields: vec![(field, rule)],
        }
    }

    /// Adds the bounds of one more field.
    pub(crate) fn add(&mut self, field: SecretFieldName, rule: SecretFieldRule) {
        self.fields.push((field, rule));
    }

    /// Returns the first field, in order, whose rules a value of `chars`
    /// characters breaks, `has_control` when it holds a control
    /// character, and why.
    pub(crate) fn broken_by(
        &self,
        chars: u64,
        has_control: bool,
    ) -> Option<(&(SecretFieldName, SecretFieldRule), InvalidValueReason)> {
        self.fields.iter().find_map(|field| {
            field
                .1
                .check(chars, has_control)
                .err()
                .map(|reason| (field, reason))
        })
    }

    /// Returns `preferred` clamped to every field's bounds, at least 1, or
    /// the first field whose bounds cannot hold together with the others.
    pub(crate) fn clamp(&self, preferred: u64) -> Result<u64, &(SecretFieldName, SecretFieldRule)> {
        let low = self
            .fields
            .iter()
            .map(|(_, rule)| rule.min_length)
            .max()
            .unwrap_or(0)
            .max(1);
        let high = self
            .fields
            .iter()
            .filter_map(|(_, rule)| rule.max_length)
            .min()
            .unwrap_or(u64::MAX);
        let length = preferred.clamp(low, high.max(low));
        // A generated value is `[A-Za-z0-9]`, so only its length can break
        // a field.
        match self.broken_by(length, false) {
            Some((broken, _)) => Err(broken),
            None => Ok(length),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str) -> SecretFieldName {
        name.parse().unwrap()
    }

    #[test]
    fn a_generated_length_is_clamped_to_every_field_and_never_below_one() {
        let mut bounds = Bounds::single(field("a"), SecretFieldRule::new(40, Some(64), None));
        assert_eq!(bounds.clamp(32), Ok(40));
        bounds.add(field("b"), SecretFieldRule::new(0, Some(48), None));
        assert_eq!(bounds.clamp(32), Ok(40));
        assert_eq!(bounds.clamp(100), Ok(48));
        assert_eq!(Bounds::default().clamp(32), Ok(32));
        assert_eq!(
            Bounds::single(field("c"), SecretFieldRule::new(0, Some(12), None)).clamp(32),
            Ok(12)
        );
        assert_eq!(
            Bounds::single(field("d"), SecretFieldRule::new(0, Some(0), None)).clamp(32),
            Err(&(field("d"), SecretFieldRule::new(0, Some(0), None)))
        );
        let clash = Bounds {
            fields: vec![
                (field("e"), SecretFieldRule::new(20, Some(30), None)),
                (field("f"), SecretFieldRule::new(0, Some(10), None)),
            ],
        };
        assert!(clash.clamp(32).is_err());
    }

    fn broken(bounds: &Bounds, chars: u64, has_control: bool) -> Option<(&str, &'static str)> {
        bounds
            .broken_by(chars, has_control)
            .map(|((name, _), reason)| (name.as_str(), reason.as_code()))
    }

    #[test]
    fn a_value_breaks_the_first_field_it_does_not_fit() {
        let bounds = Bounds {
            fields: vec![
                (field("a"), SecretFieldRule::new(8, Some(64), None)),
                (field("b"), SecretFieldRule::new(12, None, None)),
            ],
        };
        assert_eq!(broken(&bounds, 7, false), Some(("a", "length")));
        assert_eq!(broken(&bounds, 10, false), Some(("b", "length")));
        assert_eq!(broken(&bounds, 12, false), None);
        assert_eq!(broken(&bounds, 65, false), Some(("a", "length")));
        // Neither field is printable only, so control characters pass.
        assert_eq!(broken(&bounds, 12, true), None);
    }

    #[test]
    fn the_check_takes_the_length_first_then_control_characters_of_a_printable_only_field() {
        let printable = SecretFieldRule::new(8, Some(16), None).with_printable_only(true);
        assert!(printable.printable_only);
        assert!(!SecretFieldRule::new(8, Some(16), None).printable_only);
        assert_eq!(printable.check(12, false), Ok(()));
        assert_eq!(
            printable.check(12, true),
            Err(InvalidValueReason::ControlCharacters)
        );
        assert_eq!(printable.check(7, true), Err(InvalidValueReason::Length));
        assert_eq!(printable.check(17, true), Err(InvalidValueReason::Length));
        assert_eq!(printable.check(17, false), Err(InvalidValueReason::Length));
        let open = SecretFieldRule::new(0, None, None);
        assert_eq!(open.check(u64::MAX, true), Ok(()));
        assert_eq!(printable.with_printable_only(false).check(12, true), Ok(()));
        assert_eq!(InvalidValueReason::Length.as_code(), "length");
        assert_eq!(
            InvalidValueReason::ControlCharacters.to_string(),
            "control_characters"
        );
    }

    #[test]
    fn the_fields_are_checked_in_order_each_by_length_then_control_characters() {
        let bounds = Bounds {
            fields: vec![
                (
                    field("a"),
                    SecretFieldRule::new(0, Some(64), None).with_printable_only(true),
                ),
                (field("b"), SecretFieldRule::new(20, None, None)),
            ],
        };
        assert_eq!(broken(&bounds, 10, true), Some(("a", "control_characters")));
        assert_eq!(broken(&bounds, 10, false), Some(("b", "length")));
        assert_eq!(broken(&bounds, 70, true), Some(("a", "length")));
        assert_eq!(broken(&bounds, 30, false), None);
    }
}
