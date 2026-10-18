//! Enums with a stable snake_case code for the registry and DTOs.

/// Defines a fieldless enum whose every variant has a stable snake_case
/// code: `ALL`, `as_code()`, `Display` through the code, and `FromStr` that
/// accepts only the codes, rejecting anything else with a [`ValueError`] of
/// the given [`ValueKind`].
///
/// Changing a code is a contract change that needs a registry migration.
///
/// [`ValueError`]: crate::ValueError
/// [`ValueKind`]: crate::ValueKind
macro_rules! code_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident: $kind:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $code:literal,
            )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )+
        }

        impl $name {
            /// Every value, in code order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Returns the stable snake_case code.
            #[must_use]
            pub const fn as_code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)+
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_code())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::ValueError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|value| value.as_code() == s)
                    .ok_or($crate::ValueError::new(
                        $crate::ValueKind::$kind,
                        "must be one of its snake_case codes",
                    ))
            }
        }
    };
}

pub(crate) use code_enum;
