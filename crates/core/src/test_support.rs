//! Helpers the unit tests of several core modules share.

use std::path::Path;

use stackly_domain::{
    Arch, InstanceId, InstanceManifest, ManifestRuntime, NewInstanceManifest, OperationId, Os,
    ProviderRevisionKey, RuntimeId, Target,
};
use stackly_platform::Platform;
use stackly_testkit::{FakePlatform, TestRoot};

use crate::{PreparedLayout, daemon_info, prepare_layout};

const DIGEST: &str = "da578821d15e9c0f59eb8e4672962c6f032074e802684576debc6dfc7f86815f";

/// A fake platform on `root` with its layout prepared.
pub(crate) fn prepared(root: &TestRoot) -> (FakePlatform, PreparedLayout) {
    let fake = FakePlatform::new(root);
    let prepared = prepare_layout(&fake.platform()).expect("a fresh test root prepares");
    (fake, prepared)
}

/// The manifest a create of instance `id` would write, with its times from
/// the platform clock and its version from [`daemon_info`].
pub(crate) fn manifest_for(platform: &Platform, id: InstanceId) -> InstanceManifest {
    InstanceManifest::new(NewInstanceManifest {
        instance_id: id,
        name: "orders-db".parse().expect("orders-db is an instance name"),
        display_name: None,
        software: "postgresql".parse().expect("postgresql is a software name"),
        provider: ProviderRevisionKey::new(
            "official/postgresql"
                .parse()
                .expect("official/postgresql is a provider name"),
            "2026.10.1".parse().expect("2026.10.1 is a revision label"),
            DIGEST.parse().expect("DIGEST is 64 hex characters"),
        ),
        runtime: ManifestRuntime::managed(
            RuntimeId::generate(),
            "17.6".parse().expect("17.6 is an exact version"),
            "17".parse().expect("17 is a runtime line"),
            Target::new(Os::Macos, Arch::Aarch64),
            DIGEST.parse().expect("DIGEST is 64 hex characters"),
        ),
        created_at: platform.clock().now(),
        created_by_operation: OperationId::generate(),
        stackly_version: daemon_info().version.to_owned(),
    })
}

/// Another valid ID with the same short key as `id`: only the first
/// character, part of the UUID v7 timestamp, differs.
pub(crate) fn same_short_key(id: InstanceId) -> InstanceId {
    let text = id.to_string();
    let first = if text.starts_with('0') { "1" } else { "0" };
    let other: InstanceId = format!("{first}{}", &text[1..])
        .parse()
        .expect("changing the timestamp keeps a UUID v7");
    assert_eq!(other.short_key(), id.short_key());
    other
}

/// The names of the temporary files an atomic write would leave in `dir`.
pub(crate) fn temp_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("the folder exists")
        .map(|entry| {
            entry
                .expect("the entry is readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.contains(".tmp-"))
        .collect()
}
