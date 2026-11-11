//! Local provider packages (spec 0037): the import rules, the package
//! digest an approval binds, the permission summary shown before an
//! approval, and the loader of the stored copies into catalog entries.
//!
//! Nothing here writes a file, runs a process, touches the network, or
//! resolves a secret: the rules are pure functions over a validated
//! package and the registry rows a caller read, and the loader only reads.

mod digest;
mod load;
mod permissions;
mod rules;

pub use digest::{package_bytes, package_sha256};
pub use load::load;
pub use permissions::{
    DOWNLOAD_HOSTS_MAX, ExecutableUse, PERMISSIONS_SCHEMA, PermissionChange, Permissions,
    PermissionsJsonError, PermittedDownload, PermittedEndpoint, PermittedExecutable,
    PermittedOpenWith, permission_changes, permissions,
};
pub use rules::{
    IMPORT_PATH_MAX, ImportBlockReason, ImportCheck, ImportLimit, ImportPathError, LOCAL_BYTES_MAX,
    LOCAL_PROVIDERS_MAX, STACKLY_NAMESPACE, catalog_id_issue, check_import_path, check_importable,
    import_limit,
};
