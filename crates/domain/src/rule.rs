//! The business rules of the PRD, BR-001 to BR-025.

use std::fmt;
use std::str::FromStr;

use crate::error::{ValueError, ValueKind};

/// Defines [`Rule`] with one variant per business rule and its stable ID.
macro_rules! rules {
    ($($variant:ident => $id:literal),+ $(,)?) => {
        /// A business rule of the PRD, BR-001 to BR-025.
        ///
        /// Errors that reject an input because of a business rule return it
        /// through their `rule()` method, so diagnostics and evidence records
        /// link back to the PRD.
        ///
        /// ```
        /// use stackly_domain::Rule;
        ///
        /// assert_eq!(Rule::Br002.id(), "BR-002");
        /// assert_eq!(Rule::ALL.len(), 25);
        /// ```
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Rule {
            $(
                #[doc = concat!("Business rule ", $id, ".")]
                $variant,
            )+
        }

        impl Rule {
            /// Every rule, in ID order.
            pub const ALL: [Self; 25] = [$(Self::$variant),+];

            /// Returns the stable PRD ID, for example `BR-002`.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self {
                    $(Self::$variant => $id,)+
                }
            }
        }
    };
}

rules! {
    Br001 => "BR-001",
    Br002 => "BR-002",
    Br003 => "BR-003",
    Br004 => "BR-004",
    Br005 => "BR-005",
    Br006 => "BR-006",
    Br007 => "BR-007",
    Br008 => "BR-008",
    Br009 => "BR-009",
    Br010 => "BR-010",
    Br011 => "BR-011",
    Br012 => "BR-012",
    Br013 => "BR-013",
    Br014 => "BR-014",
    Br015 => "BR-015",
    Br016 => "BR-016",
    Br017 => "BR-017",
    Br018 => "BR-018",
    Br019 => "BR-019",
    Br020 => "BR-020",
    Br021 => "BR-021",
    Br022 => "BR-022",
    Br023 => "BR-023",
    Br024 => "BR-024",
    Br025 => "BR-025",
}

impl FromStr for Rule {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|rule| rule.id() == s)
            .ok_or(ValueError::new(
                ValueKind::Rule,
                "must be a known business rule ID such as BR-002",
            ))
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_ids_count_from_br_001_to_br_025_in_order() {
        let ids: Vec<&str> = Rule::ALL.iter().map(|rule| rule.id()).collect();
        let expected: Vec<String> = (1..=25).map(|n| format!("BR-{n:03}")).collect();

        assert_eq!(ids, expected);
        assert_eq!(Rule::Br023.to_string(), "BR-023");
    }

    #[test]
    fn rule_ids_parse_back_and_unknown_ids_are_rejected() {
        for rule in Rule::ALL {
            assert_eq!(rule.id().parse::<Rule>().unwrap(), rule);
        }
        for bad in [
            "", "BR-000", "BR-026", "br-001", "BR-1", "VAL-007", "BR-001 ",
        ] {
            assert!(bad.parse::<Rule>().is_err(), "{bad:?}");
        }
    }
}
