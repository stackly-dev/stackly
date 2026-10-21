//! Sources to duplicate: instances on a [`Site`] whose pinned package is in
//! the data folder, with an active config revision and secret uses.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use stackly_core::instance::duplicate::{DuplicateDeps, DuplicateFacts, gather_duplicate};
use stackly_domain::Registry;
use stackly_domain::{
    ConfigRevisionId, ConfigRevisionRecord, ConfigValuesJson, Instance, InstanceLifecycle,
    PlatformStoreKey, SecretFieldName, SecretOwner, SecretPurpose, SecretRef, SecretReferenceId,
    SecretReferenceRecord, SecretUser, WriteBatch, WriteOp,
};
use stackly_platform::Clock;

use crate::common::{Site, provider_dir};

/// Copies the provider package `name`, edited by `edit` like the package
/// the site was built with, to the folder its instances pin.
pub fn pin_package(site: &Site, name: &str, edit: impl FnOnce(&Path)) {
    let target = site
        .prepared
        .layout()
        .provider_revision_dir(site.package.key());
    copy_dir(&provider_dir(name), &target);
    edit(&target);
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the target folder can be made");
    for entry in fs::read_dir(from).expect("the source folder reads") {
        let entry = entry.expect("an entry reads");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the type reads").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a file copies");
        }
    }
}

/// Commits a stopped instance of the site, named `name`.
pub fn add_source(site: &Site, name: &str) -> Instance {
    site.add_instance(|builder| builder.name(name).lifecycle(InstanceLifecycle::Stopped))
}

/// Commits `values` as the active config revision of `instance`.
pub fn set_config(site: &Site, instance: &Instance, values: &serde_json::Value) {
    let revision = ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        instance.id(),
        ConfigValuesJson::new(values.to_string()).expect("the values are object text"),
        None,
        site.fake.clock().now(),
    );
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertConfigRevision(
            revision,
        )]))
        .expect("the config revision commits");
}

/// Commits `instance` as the user of the reference it owns for `field` and
/// returns the reference.
pub fn use_secret(site: &Site, instance: &Instance, field: &str) -> SecretRef {
    let purpose = SecretPurpose::from_field_name(field).expect("a valid purpose");
    let reference = SecretRef::for_instance(instance.id(), &purpose);
    let record = SecretReferenceRecord::new(
        SecretReferenceId::generate(),
        reference.clone(),
        PlatformStoreKey::new(format!("fake:{reference}")).expect("a valid key"),
        SecretOwner::Instance(instance.id()),
        purpose,
        site.fake.clock().now(),
    );
    let uses: BTreeMap<SecretFieldName, SecretRef> =
        [(field.parse().expect("a field name"), reference.clone())]
            .into_iter()
            .collect();
    site.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertSecretReference(record),
            WriteOp::ReplaceSecretUses {
                user: SecretUser::Instance(instance.id()),
                uses,
            },
        ]))
        .expect("the secret use commits");
    reference
}

/// The dependencies of `site` with its own catalog.
pub fn deps(site: &Site) -> DuplicateDeps<'_> {
    DuplicateDeps {
        registry: &site.registry,
        catalog: &site.catalog,
        platform: &site.platform,
        prepared: &site.prepared,
    }
}

/// The facts of a duplicate of `instance`, which must be readable.
pub fn facts(site: &Site, instance: &Instance) -> DuplicateFacts {
    gather_duplicate(&deps(site), instance.id())
        .unwrap_or_else(|error| panic!("the facts gather: {error}"))
}

/// Sets `multiInstance` of a copy of the provider `fake` to false.
pub fn single_instance(dir: &Path) {
    let path = dir.join("provider.yaml");
    let text = fs::read_to_string(&path).expect("the manifest reads");
    assert!(text.contains("multiInstance: true"));
    fs::write(
        &path,
        text.replace("multiInstance: true", "multiInstance: false"),
    )
    .expect("the manifest writes");
}

/// Changes the description of a copy of a provider, which changes its
/// digest and keeps its revision label.
pub fn change_digest(dir: &Path) {
    let path = dir.join("provider.yaml");
    let mut text = fs::read_to_string(&path).expect("the manifest reads");
    text.push_str("\n# another digest\n");
    fs::write(&path, text).expect("the manifest writes");
}
