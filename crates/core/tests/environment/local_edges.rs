//! `.stackly.local.yaml` at its edges (spec 0045): the 64 KiB limit and
//! the file kind of the read, anchors and aliases that give the same plan
//! as the plain text while a merge key or a local tag never applies, the
//! bounds of a port and of a data location, and K22 for a storage override
//! and for a file whose shape fails.

use std::fs;

use stackly_core::environment::local::{
    LOCAL_MAX_LEN, LocalFile, LocalInput, LocalState, OverrideState,
};
use stackly_core::environment::{Plan, UnreadableReason};
use stackly_core::storage::prepare_location;
use stackly_domain::{
    InstanceLifecycle, LocationKind, PortRequest, Registry, StorageLocation, StorageLocationId,
    WriteBatch, WriteOp,
};
use stackly_platform::Clock;

use crate::site::{Inputs, Site};

const SHOP: &str = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  api:
    provider: testing/fake
    version: \"0.1\"
    endpoints:
      http: {port: 18080}
  db:
    provider: official/postgresql
    version: \"18\"
    endpoints:
      client: {port: 15432}
  borrowed:
    provider: testing/fake
    ownership: shared
    instanceRef: main-api
";

fn local(text: &str) -> LocalInput {
    LocalInput {
        file: ".stackly.local.yaml".into(),
        content: LocalFile::from_read(Ok(text.as_bytes().to_vec())),
    }
}

fn overrides(body: &str) -> String {
    format!("apiVersion: stackly/v1alpha1\nkind: EnvironmentLocal\nservices:\n{body}")
}

fn plan(site: &Site, local: &LocalInput) -> Plan {
    site.plan_with(
        SHOP.as_bytes(),
        &site.location("stackly.yaml"),
        None,
        &Inputs {
            local: Some(local),
            ..Inputs::default()
        },
    )
}

fn site() -> Site {
    let mut site = Site::new();
    site.instance("main-api", 18090, InstanceLifecycle::Running);
    site
}

fn finding_codes(plan: &Plan) -> Vec<&'static str> {
    plan.local
        .as_ref()
        .expect("a local file")
        .findings
        .list()
        .iter()
        .map(|f| f.code().as_code())
        .collect()
}

fn codes(plan: &Plan) -> Vec<&'static str> {
    plan.blockers.iter().map(|b| b.kind.code()).collect()
}

fn port_of(plan: &Plan, alias: &str) -> Vec<PortRequest> {
    plan.items
        .iter()
        .find(|item| item.alias.as_str() == alias)
        .expect("the item")
        .endpoints
        .iter()
        .map(|e| e.port)
        .collect()
}

fn fixed(port: u16) -> PortRequest {
    PortRequest::Fixed(stackly_domain::Port::new(port).expect("a port"))
}

// covers: AC-9 (Nama file): an override file is read up to 64 KiB, a byte
// more is `env.local_unreadable`, and a symlink at its path is never
// followed
#[test]
fn an_override_file_reads_up_to_64_kib_and_a_larger_file_or_a_symlink_is_unreadable() {
    let site = site();
    let dir = site.root.path().join("project");
    fs::create_dir_all(&dir).expect("the folder");
    let path = dir.join(".stackly.local.yaml");
    let text = overrides("  api:\n    endpoints:\n      http: {port: 18100}\n");
    let limit = usize::try_from(LOCAL_MAX_LEN).expect("fits");
    let at_limit = format!("{text}# {}\n", "x".repeat(limit - text.len() - 3));
    assert_eq!(at_limit.len(), limit);
    fs::write(&path, &at_limit).expect("writes");
    let read = LocalInput {
        file: ".stackly.local.yaml".into(),
        content: LocalFile::read(site.platform.fs(), &path),
    };
    let planned = plan(&site, &read);
    assert!(planned.applicable, "{:#?}", planned.blockers);
    assert_eq!(port_of(&planned, "api"), [fixed(18100)]);

    fs::write(&path, format!("{at_limit}#")).expect("writes");
    let content = LocalFile::read(site.platform.fs(), &path);
    assert!(matches!(
        content,
        LocalFile::Unreadable(UnreadableReason::TooLarge)
    ));
    let too_large = plan(
        &site,
        &LocalInput {
            file: ".stackly.local.yaml".into(),
            content,
        },
    );
    assert_eq!(codes(&too_large), ["env.local_unreadable"]);
    assert_eq!(
        too_large.local.as_ref().map(|l| l.state),
        Some(LocalState::Unreadable)
    );
    assert_eq!(port_of(&too_large, "api"), [fixed(18080)], "the manifest");

    let target = dir.join("elsewhere.yaml");
    fs::write(&target, &text).expect("writes");
    fs::remove_file(&path).expect("removes");
    std::os::unix::fs::symlink(&target, &path).expect("links");
    assert!(matches!(
        LocalFile::read(site.platform.fs(), &path),
        LocalFile::Unreadable(UnreadableReason::NotAFile)
    ));
    fs::remove_file(&path).expect("removes the link");
    assert!(matches!(
        LocalFile::read(site.platform.fs(), &path),
        LocalFile::Absent
    ));
}

// covers: AC-9 (Aturan override, `yaml_alias`): an anchor and its alias
// give the same overrides as the plain text; a merge key or a local tag is
// one `yaml_alias` error and no override applies, while a tag the parser
// resolves is no finding
#[test]
fn anchors_and_aliases_apply_like_plain_text_while_a_merge_key_or_a_local_tag_never_does() {
    let site = site();
    let plain = plan(
        &site,
        &local(&overrides(
            "  api:\n    endpoints:\n      http: {port: auto}\n  db:\n    endpoints:\n      client: {port: auto}\n",
        )),
    );
    let aliased = plan(
        &site,
        &local(&overrides(
            "  api:\n    endpoints:\n      http: &same {port: auto}\n  db:\n    endpoints:\n      client: *same\n",
        )),
    );
    let fields = |plan: &Plan| {
        plan.local
            .as_ref()
            .expect("a file")
            .overrides
            .iter()
            .map(|o| (o.service.clone(), o.field.clone(), o.value.clone(), o.state))
            .collect::<Vec<_>>()
    };
    assert_eq!(fields(&aliased), fields(&plain));
    assert!(
        fields(&aliased)
            .iter()
            .all(|(.., state)| *state == OverrideState::Applied)
    );
    assert_eq!(port_of(&aliased, "db"), [PortRequest::Auto]);
    assert_eq!(port_of(&aliased, "api"), port_of(&plain, "api"));

    let resolved = plan(
        &site,
        &local(&overrides(
            "  api:\n    endpoints:\n      http: {port: !!int 18100}\n",
        )),
    );
    assert!(resolved.applicable, "{:#?}", resolved.blockers);
    assert_eq!(port_of(&resolved, "api"), [fixed(18100)]);

    for body in [
        "  api:\n    <<: {endpoints: {http: {port: 18100}}}\n",
        "  api:\n    endpoints:\n      http: {port: !port 18100}\n",
        "  api: !<!svc>\n    endpoints:\n      http: {port: 18100}\n",
    ] {
        let refused = plan(&site, &local(&overrides(body)));
        assert_eq!(finding_codes(&refused), ["yaml_alias"], "{body}");
        assert_eq!(codes(&refused), ["env.local_invalid"]);
        assert_eq!(port_of(&refused, "api"), [fixed(18080)], "{body}");
        let file = refused.local.as_ref().expect("a file");
        assert_eq!(file.state, LocalState::Invalid);
        assert!(file.overrides.is_empty(), "a failed shape lists nothing");
        assert!(file.sha256.is_some());
    }
}

// covers: AC-9 (Schema `.stackly.local.yaml`): `port` takes 1 to 65535 or
// `auto` and nothing else, `services` takes at most 32 entries, and
// `dataLocation` is an absolute path of at most 1024 bytes without NUL or
// a `.` or `..` segment
#[test]
fn ports_services_and_data_locations_accept_their_bounds_and_refuse_one_past() {
    let site = site();
    for (port, expected) in [
        ("1", fixed(1)),
        ("65535", fixed(65535)),
        ("auto", PortRequest::Auto),
    ] {
        let planned = plan(
            &site,
            &local(&overrides(&format!(
                "  api:\n    endpoints:\n      http: {{port: {port}}}\n"
            ))),
        );
        assert!(finding_codes(&planned).is_empty(), "{port}");
        assert_eq!(port_of(&planned, "api"), [expected], "{port}");
    }
    for port in [
        "0",
        "65536",
        "-1",
        "\"18100\"",
        "\"AUTO\"",
        "18100.5",
        "null",
    ] {
        let planned = plan(
            &site,
            &local(&overrides(&format!(
                "  api:\n    endpoints:\n      http: {{port: {port}}}\n"
            ))),
        );
        assert_eq!(codes(&planned), ["env.local_invalid"], "{port}");
        assert_eq!(port_of(&planned, "api"), [fixed(18080)], "{port}");
    }

    // 32 aliases the manifest does not have are 32 warnings; 33 is an
    // error of the whole map.
    let ghosts = |count: usize| {
        (0..count)
            .map(|at| format!("  ghost{at:02}:\n    endpoints:\n      http: {{port: 18100}}\n"))
            .collect::<String>()
    };
    let thirty_two = plan(&site, &local(&overrides(&ghosts(32))));
    assert!(thirty_two.applicable, "{:#?}", thirty_two.blockers);
    assert_eq!(finding_codes(&thirty_two).len(), 32);
    let thirty_three = plan(&site, &local(&overrides(&ghosts(33))));
    assert_eq!(codes(&thirty_three), ["env.local_invalid"]);
    assert_eq!(finding_codes(&thirty_three), ["invalid_value"]);

    let long = format!("/{}", "a".repeat(1023));
    let too_long = format!("/{}", "a".repeat(1024));
    for (path, refused) in [
        (long.as_str(), false),
        (too_long.as_str(), true),
        ("\"/Volumes/a\\0b\"", true),
        ("/Volumes/./Fast", true),
        ("/Volumes/Fast/..", true),
        ("~/Fast", true),
    ] {
        let planned = plan(
            &site,
            &local(&overrides(&format!(
                "  db:\n    storage:\n      dataLocation: {path}\n"
            ))),
        );
        let found = finding_codes(&planned);
        if refused {
            assert_eq!(found, ["storage_location_invalid"], "{path}");
            assert_eq!(codes(&planned), ["env.local_invalid"], "{path}");
        } else {
            // A good path that is no registered location blocks later.
            assert!(found.is_empty(), "{path}: {found:?}");
            assert_eq!(codes(&planned), ["env.override_location_unknown"]);
        }
    }
}

// covers: AC-9, AC-11 (K22): a good storage override in a file with an
// error is `ignored` and gives no `PlanItem.storage`, while a file with
// only a warning still applies it
#[test]
fn a_good_storage_override_beside_an_error_is_ignored_and_beside_a_warning_applies() {
    let site = site();
    let folder = site.root.path().join("fast");
    fs::create_dir_all(&folder).expect("the folder");
    let id = StorageLocationId::generate();
    let made = prepare_location(&site.platform, &folder, LocationKind::Data, id)
        .expect("the location folders");
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertStorageLocation(
            StorageLocation::custom(
                id,
                LocationKind::Data,
                made.to_str().expect("text"),
                site.fake.clock().now(),
            )
            .expect("a location"),
        )]))
        .expect("commits");
    let canonical = fs::canonicalize(&folder).expect("canonical");
    let storage = format!(
        "  db:\n    storage:\n      dataLocation: {}\n",
        canonical.display()
    );
    let db_storage = |plan: &Plan| {
        plan.items
            .iter()
            .find(|item| item.alias.as_str() == "db")
            .expect("db")
            .storage
            .clone()
    };

    let with_error = plan(
        &site,
        &local(&overrides(&format!(
            "{storage}  api:\n    endpoints:\n      http: {{port: 0}}\n"
        ))),
    );
    assert_eq!(codes(&with_error), ["env.local_invalid"]);
    let file = with_error.local.as_ref().expect("a file");
    assert_eq!(file.state, LocalState::Invalid);
    assert!(
        file.overrides
            .iter()
            .all(|o| o.state == OverrideState::Ignored),
        "{:#?}",
        file.overrides
    );
    assert!(
        file.overrides
            .iter()
            .any(|o| o.field.contains("dataLocation")),
        "the storage request is still listed: {:#?}",
        file.overrides
    );
    assert!(db_storage(&with_error).is_none(), "the default location");

    let with_warning = plan(
        &site,
        &local(&overrides(&format!(
            "{storage}  ghost:\n    endpoints:\n      http: {{port: 18100}}\n"
        ))),
    );
    assert!(with_warning.applicable, "{:#?}", with_warning.blockers);
    let file = with_warning.local.as_ref().expect("a file");
    assert_eq!(file.state, LocalState::Present);
    let states: Vec<(&str, OverrideState)> = file
        .overrides
        .iter()
        .map(|o| (o.service.as_str(), o.state))
        .collect();
    assert!(
        states.contains(&("db", OverrideState::Applied)),
        "{states:?}"
    );
    assert!(
        states.contains(&("ghost", OverrideState::Ignored)),
        "{states:?}"
    );
    assert_eq!(db_storage(&with_warning).map(|s| s.location_id), Some(id));
}
