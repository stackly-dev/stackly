//! The `secret.<field>`, `data`, and `disk` rows (spec 0020 AC-6, AC-7).

use std::sync::Arc;

use stackly_core::instance::create::{
    CheckStatus, CreateError, DirFact, ProbeFact, SecretSourceKind, SecretValueShape, SourceFact,
};
use stackly_core::secret::InvalidValueReason;
use stackly_domain::{
    PlatformStoreKey, Registry, SecretOwner, SecretPurpose, SecretRef, SecretReferenceId,
    SecretReferenceRecord, WriteBatch, WriteOp,
};
use stackly_evidence::{Kind, evidence};
use stackly_platform::{Clock, SecretBackend, StoreState};
use stackly_testkit::FakeVolume;

use crate::common::{Site, error_codes, paths, row, source};

fn register(site: &Site, text: &str) -> SecretRef {
    let reference = SecretRef::parse(text).expect("a valid reference");
    let record = SecretReferenceRecord::new(
        SecretReferenceId::generate(),
        reference.clone(),
        PlatformStoreKey::new(format!("fake:{text}")).expect("a valid key"),
        SecretOwner::User,
        SecretPurpose::user(),
        site.fake.clock().now(),
    );
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertSecretReference(
            record,
        )]))
        .expect("the reference commits");
    reference
}

#[test]
fn a_secret_field_defaults_to_generate_and_shows_its_source() {
    let site = Site::new();
    let mut draft = site.draft("orders");

    let evaluation = site.evaluate(&draft);
    let secret = row(&evaluation, "secret.authToken");
    assert_eq!(secret.status, CheckStatus::Ok);
    assert_eq!(secret.title, "Auth token stored in Keychain");
    assert_eq!(secret.detail, "Generated on create, never logged");

    draft.secrets = vec![source("authToken", SecretSourceKind::Value)];
    let evaluation = site.evaluate(&draft);
    assert_eq!(
        row(&evaluation, "secret.authToken").detail,
        "Entered value, never logged"
    );
}

#[test]
fn a_value_that_breaks_the_rules_of_its_field_is_secret_invalid_value() {
    let ev = evidence!(
        covers = ["VAL-012"],
        kind = Kind::Fixture,
        expected = "a value of a secret field outside minLength and maxLength, and a value with a control character for a printableOnly field, make secret.invalid_value with reason length or control_characters at secrets[i].input.content",
    );
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.secrets = vec![source("authToken", SecretSourceKind::Value)];
    draft.secret_values = vec![(
        "authToken".to_owned(),
        SecretValueShape {
            chars: 300,
            has_control: false,
        },
    )];

    let evaluation = site.evaluate(&draft);

    assert_eq!(error_codes(&evaluation), ["secret.invalid_value"]);
    assert_eq!(
        evaluation.first_error().cloned(),
        Some(CreateError::SecretInvalidValue {
            field: "authToken".to_owned(),
            reason: InvalidValueReason::Length
        })
    );
    assert_eq!(
        paths(&evaluation),
        [("secrets[0].input.content", "invalid_value")]
    );

    let postgres = Site::with_provider("postgresql");
    let mut draft = postgres.draft("orders");
    draft.secrets = vec![source("superuserPassword", SecretSourceKind::Value)];
    draft.secret_values = vec![(
        "superuserPassword".to_owned(),
        SecretValueShape {
            chars: 12,
            has_control: true,
        },
    )];
    let evaluation = postgres.evaluate(&draft);
    assert_eq!(
        evaluation.first_error().cloned(),
        Some(CreateError::SecretInvalidValue {
            field: "superuserPassword".to_owned(),
            reason: InvalidValueReason::ControlCharacters
        })
    );
    // The same value with no control character passes.
    draft.secret_values = vec![(
        "superuserPassword".to_owned(),
        SecretValueShape {
            chars: 12,
            has_control: false,
        },
    )];
    assert!(postgres.evaluate(&draft).ready());
    ev.pass();
}

#[test]
fn copy_from_and_bind_need_a_registered_reference_with_a_stored_value() {
    let ev = evidence!(
        covers = ["VAL-012"],
        kind = Kind::Fixture,
        expected = "a source reference that does not parse is secret.invalid_reference, one no row names is secret.not_found, one without a stored value is secret.needs_secret, and one with a value is ok with its own copy",
    );
    let site = Site::new();
    let known = register(&site, "local:shared/pg/admin");
    let mut draft = site.draft("orders");

    draft.secrets = vec![source(
        "authToken",
        SecretSourceKind::CopyFrom("not a reference".to_owned()),
    )];
    assert_eq!(
        error_codes(&site.evaluate(&draft)),
        ["secret.invalid_reference"]
    );

    draft.secrets = vec![source(
        "authToken",
        SecretSourceKind::CopyFrom("local:shared/pg/other".to_owned()),
    )];
    assert_eq!(error_codes(&site.evaluate(&draft)), ["secret.not_found"]);

    draft.secrets = vec![source(
        "authToken",
        SecretSourceKind::Bind(known.to_string()),
    )];
    let evaluation = site.evaluate(&draft);
    assert_eq!(error_codes(&evaluation), ["secret.needs_secret"]);
    assert_eq!(
        row(&evaluation, "secret.authToken").detail,
        "local:shared/pg/admin has no stored value yet"
    );

    site.fake.secrets().insert(&known, "stored-value-12345");
    let evaluation = site.evaluate(&draft);
    assert!(evaluation.ready(), "{:#?}", evaluation.checks);
    assert_eq!(
        row(&evaluation, "secret.authToken").detail,
        "Uses local:shared/pg/admin"
    );
    let resolved = evaluation.resolved.expect("ready");
    assert_eq!(
        resolved.secrets[0].reference, known,
        "a bind keeps its reference"
    );

    draft.secrets = vec![source(
        "authToken",
        SecretSourceKind::CopyFrom(known.to_string()),
    )];
    let evaluation = site.evaluate(&draft);
    assert_eq!(
        row(&evaluation, "secret.authToken").detail,
        "Copied from local:shared/pg/admin"
    );
    let resolved = evaluation.resolved.expect("ready");
    assert_ne!(resolved.secrets[0].reference, known, "a copy gets its own");
    ev.pass();
}

#[test]
fn a_locked_or_busy_store_is_a_caution_and_an_unsupported_store_is_an_error() {
    let site = Site::new();
    let draft = site.draft("orders");

    let mut facts = site.facts(&draft);
    facts.secrets.state = StoreState::Locked;
    let evaluation = stackly_core::instance::create::evaluate(&draft, &facts);
    assert!(evaluation.ready());
    let secret = row(&evaluation, "secret.authToken");
    assert_eq!(secret.status, CheckStatus::Caution);
    assert_eq!(
        secret.detail,
        "Keychain is locked; macOS asks you to unlock it when you create"
    );

    let mut draft = site.draft("orders");
    draft.secrets = vec![source(
        "authToken",
        SecretSourceKind::CopyFrom("local:shared/pg/admin".to_owned()),
    )];
    register(&site, "local:shared/pg/admin");
    let mut facts = site.facts(&draft);
    facts.secrets.sources.insert(
        "local:shared/pg/admin".to_owned(),
        SourceFact::Registered(ProbeFact::Busy),
    );
    let evaluation = stackly_core::instance::create::evaluate(&draft, &facts);
    let secret = row(&evaluation, "secret.authToken");
    assert_eq!(secret.status, CheckStatus::Caution);
    assert_eq!(
        secret.detail,
        "Keychain is busy; checked again when you create"
    );

    facts.secrets.backend = SecretBackend::Unsupported;
    let evaluation = stackly_core::instance::create::evaluate(&draft, &facts);
    assert_eq!(error_codes(&evaluation), ["secret.store_unsupported"]);
}

#[test]
fn the_data_row_needs_private_folders_and_a_local_apfs_or_hfs_volume() {
    let ev = evidence!(
        covers = ["VAL-008"],
        kind = Kind::Fixture,
        expected = "a data root on a network or other file system is instance.storage_unsupported with the type, and an instances folder that is not private is supervisor.data_unavailable with a relative path",
    );
    let site = Site::new();
    let draft = site.draft("orders");
    let evaluation = site.evaluate(&draft);
    assert_eq!(
        row(&evaluation, "data").detail,
        "instances/<new instance id>/data · created new for this instance"
    );

    let data_root = site.prepared.layout().roots().data().to_path_buf();
    site.fake
        .fs()
        .set_volume(&data_root, FakeVolume::DEFAULT.with_kind("smbfs", false));
    let evaluation = site.evaluate(&draft);
    assert_eq!(error_codes(&evaluation), ["instance.storage_unsupported"]);
    let Some(CreateError::StorageUnsupported { fs_type, root }) = evaluation.first_error().cloned()
    else {
        panic!("storage unsupported");
    };
    assert_eq!((fs_type.as_str(), root.as_str()), ("smbfs", "."));

    site.fake
        .fs()
        .set_volume(&data_root, FakeVolume::DEFAULT.with_kind("apfs", false));
    assert_eq!(
        error_codes(&site.evaluate(&draft)),
        ["instance.storage_unsupported"],
        "a local APFS volume is local"
    );
    site.fake
        .fs()
        .set_volume(&data_root, FakeVolume::DEFAULT.with_kind("hfs", true));
    assert!(site.evaluate(&draft).ready());

    let mut facts = site.facts(&draft);
    facts.data.instances = DirFact::Problem("the folder is open to others".to_owned());
    let evaluation = stackly_core::instance::create::evaluate(&draft, &facts);
    assert_eq!(error_codes(&evaluation), ["supervisor.data_unavailable"]);
    let Some(CreateError::DataUnavailable { path, .. }) = evaluation.first_error().cloned() else {
        panic!("data unavailable");
    };
    assert_eq!(path, "instances");
    ev.pass();
}

#[test]
fn the_disk_row_needs_256_mb_free_in_the_instances_folder() {
    let site = Site::new();
    let draft = site.draft("orders");
    let instances = site.prepared.layout().instances_dir();

    site.fake
        .fs()
        .set_volume(&instances, FakeVolume::new(1 << 40, 255_999_999));
    let evaluation = site.evaluate(&draft);
    assert_eq!(error_codes(&evaluation), ["instance.insufficient_space"]);
    assert_eq!(
        evaluation.first_error().cloned(),
        Some(CreateError::InsufficientSpace {
            needed: 256_000_000,
            available: 255_999_999
        })
    );
    assert_eq!(
        row(&evaluation, "disk").detail,
        "255 MB free · needs at least 256 MB"
    );

    site.fake
        .fs()
        .set_volume(&instances, FakeVolume::new(1 << 40, 256_000_000));
    assert!(site.evaluate(&draft).ready());
    site.fake
        .fs()
        .set_volume(&instances, FakeVolume::new(1 << 40, 41_000_000_000));
    assert_eq!(
        row(&site.evaluate(&draft), "disk").detail,
        "41 GB free · needs at least 256 MB"
    );
    let _ = Arc::strong_count(&site.package);
}
