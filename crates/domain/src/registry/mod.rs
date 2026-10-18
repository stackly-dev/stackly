//! Values the registry stores: its own identity and the stored form of
//! instance paths. The registry port itself comes with spec 0005 step 8.

mod path;

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

use crate::id::InvalidId;
use crate::text::serde_as_text;

pub use path::{InstancePathEntry, InstancePathKind, LogicalRoot, RelativePath};

/// Length of a hyphenated UUID, the only text form an ID accepts. The
/// `uuid_id!` macro from `id.rs` expects it in scope.
const HYPHENATED_LEN: usize = 36;

uuid_id! {
    /// Stable identifier of one registry file, always a UUID version 7.
    ///
    /// Only the storage adapter creates it, once, when it creates the
    /// registry; it never changes afterwards, so a restored or copied
    /// registry can be told apart from a new one.
    RegistryId
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_version_7_and_round_trip_through_text_and_serde() {
        let id = RegistryId::generate();
        let upper = id.to_string().to_uppercase();

        assert_eq!(id.as_uuid().get_version_num(), 7);
        assert_eq!(upper.parse::<RegistryId>().unwrap(), id);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<RegistryId>(&json).unwrap(), id);
        assert_eq!(
            "9f1c2d3e-4b5a-4c6d-8e7f-0a1b2c3d4e5f".parse::<RegistryId>(),
            Err(InvalidId::WrongVersion(4))
        );
    }
}
