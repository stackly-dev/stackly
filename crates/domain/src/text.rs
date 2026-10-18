//! Serde support for value types that travel in their text form.
//!
//! A value type serializes through `Display` and deserializes through
//! `FromStr`, so input from JSON goes through the same validation as any
//! other input, and a value that exists is always valid.

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

/// Deserializes a value type from a string through its `FromStr`.
pub(crate) struct FromStrVisitor<T> {
    expecting: &'static str,
    value: PhantomData<T>,
}

impl<T> FromStrVisitor<T> {
    /// Creates a visitor that names the expected input as `expecting`, for
    /// example `an instance name`.
    pub(crate) const fn new(expecting: &'static str) -> Self {
        Self {
            expecting,
            value: PhantomData,
        }
    }
}

impl<T> serde::de::Visitor<'_> for FromStrVisitor<T>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.expecting)
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<T, E> {
        value.parse().map_err(E::custom)
    }
}

/// Implements `Serialize` through `Display` and `Deserialize` through
/// `FromStr` for each listed type, with the text that names the expected
/// input in a type error.
macro_rules! serde_as_text {
    ($($ty:ty => $expecting:literal),+ $(,)?) => {$(
        impl ::serde::Serialize for $ty {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $ty {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_str($crate::text::FromStrVisitor::new($expecting))
            }
        }
    )+};
}

pub(crate) use serde_as_text;
