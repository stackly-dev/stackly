//! `environment::register` and `unregister` (spec 0044 AC-11 to AC-13):
//! the guards of `import_environment` and `remove_environment`, run inside
//! a guarded commit of the fake registry.

use stackly_core::diagnostics::HomeMask;
use stackly_core::environment::{
    ENVIRONMENT_LIMIT, EnvOpError, RegisterDecision, RegisterInput, manifest_digest, register,
    unregister,
};
use stackly_domain::{
    EnvironmentId, GuardDecision, Guarded, ManifestLocation, Registry, ResourceRevision,
};

use crate::site::Site;

fn input(site: &Site, name: &str, file: &str, bytes: &[u8]) -> RegisterInput {
    RegisterInput {
        id: EnvironmentId::generate(),
        name: name.parse().expect("a name"),
        location: site.location(file),
        sha256: manifest_digest(bytes),
        declared: Vec::new(),
        relink: None,
    }
}

/// What the guard decided, committed when it writes.
enum Outcome {
    Wrote,
    Unchanged,
    Refused(EnvOpError),
}

fn run(
    site: &Site,
    decide: impl FnOnce(&dyn stackly_domain::GuardView) -> RegisterDecision + Send + 'static,
) -> Outcome {
    let guarded = site
        .registry
        .commit_guarded(Box::new(move |view| {
            Ok(match decide(view) {
                RegisterDecision::Write(ops) => GuardDecision::Write(ops),
                RegisterDecision::Unchanged => GuardDecision::Refuse(Box::new(None::<EnvOpError>)),
                RegisterDecision::Refuse(error) => GuardDecision::Refuse(Box::new(Some(error))),
                _ => unreachable!("a decision this test does not know"),
            })
        }))
        .expect("the guarded commit runs");
    match guarded {
        Guarded::Committed(_) => Outcome::Wrote,
        Guarded::Refused(reason) => match *reason
            .downcast::<Option<EnvOpError>>()
            .expect("the test's own reason")
        {
            None => Outcome::Unchanged,
            Some(error) => Outcome::Refused(error),
        },
    }
}

fn import(site: &Site, input: &RegisterInput) -> Outcome {
    let input = input.clone();
    run(site, move |view| {
        register(view, &input, &HomeMask::none()).expect("the view reads")
    })
}

fn stored(site: &Site) -> Vec<stackly_domain::Environment> {
    site.registry.environments().expect("reads").value
}

#[test]
fn a_new_name_inserts_and_the_same_file_again_writes_nothing() {
    let site = Site::new();
    let first = input(&site, "shop", "stackly.yaml", b"one");
    assert!(matches!(import(&site, &first), Outcome::Wrote));
    let rows = stored(&site);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].revision(), ResourceRevision::INITIAL);
    assert_eq!(rows[0].manifest_location(), Some(&first.location));

    let mut again = first.clone();
    again.id = EnvironmentId::generate();
    assert!(matches!(import(&site, &again), Outcome::Unchanged));

    let mut changed = first.clone();
    changed.sha256 = manifest_digest(b"two");
    assert!(matches!(import(&site, &changed), Outcome::Wrote));
    let rows = stored(&site);
    assert_eq!(rows[0].id(), first.id, "the update keeps the ID");
    assert_eq!(rows[0].revision().get(), 2);
}

#[test]
fn a_name_at_another_location_is_taken_unless_the_request_relinks_it() {
    let site = Site::new();
    let first = input(&site, "shop", "a.yaml", b"one");
    assert!(matches!(import(&site, &first), Outcome::Wrote));

    let elsewhere = input(&site, "shop", "b.yaml", b"one");
    let Outcome::Refused(EnvOpError::NameTaken { name, location }) = import(&site, &elsewhere)
    else {
        panic!("the name is taken");
    };
    assert_eq!(name.as_str(), "shop");
    assert_eq!(location.as_deref(), Some(first.location.as_str()));

    let mut relink = elsewhere.clone();
    relink.relink = Some(first.id);
    assert!(matches!(import(&site, &relink), Outcome::Wrote));
    let rows = stored(&site);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id(), first.id);
    assert_eq!(rows[0].manifest_location(), Some(&elsewhere.location));

    let mut other = input(&site, "other", "c.yaml", b"three");
    other.relink = Some(first.id);
    assert!(matches!(
        import(&site, &other),
        Outcome::Refused(EnvOpError::RelinkMismatch { .. })
    ));
    let mut unknown = input(&site, "shop", "d.yaml", b"four");
    unknown.relink = Some(EnvironmentId::generate());
    assert!(matches!(
        import(&site, &unknown),
        Outcome::Refused(EnvOpError::NotFound)
    ));
}

#[test]
fn a_file_that_now_declares_another_name_takes_the_location_over() {
    let site = Site::new();
    let first = input(&site, "alpha", "stackly.yaml", b"one");
    assert!(matches!(import(&site, &first), Outcome::Wrote));
    let renamed = input(&site, "beta", "stackly.yaml", b"two");
    assert!(matches!(import(&site, &renamed), Outcome::Wrote));

    let rows = stored(&site);
    let alpha = rows
        .iter()
        .find(|env| env.name().as_str() == "alpha")
        .expect("kept");
    let beta = rows
        .iter()
        .find(|env| env.name().as_str() == "beta")
        .expect("added");
    assert!(
        alpha.manifest_location().is_none(),
        "alpha lets go of the file"
    );
    assert_eq!(alpha.revision().get(), 2);
    assert_eq!(beta.manifest_location(), Some(&renamed.location));
}

#[test]
fn the_registry_holds_at_most_64_environments() {
    let site = Site::new();
    for n in 0..ENVIRONMENT_LIMIT {
        let next = input(&site, &format!("env{n}"), &format!("e{n}.yaml"), b"x");
        assert!(matches!(import(&site, &next), Outcome::Wrote));
    }
    let last = input(&site, "last", "last.yaml", b"x");
    assert!(matches!(
        import(&site, &last),
        Outcome::Refused(EnvOpError::LimitReached)
    ));
    assert_eq!(stored(&site).len(), ENVIRONMENT_LIMIT);
}

#[test]
fn unregister_checks_the_revision_then_deletes() {
    let site = Site::new();
    let first = input(&site, "shop", "stackly.yaml", b"one");
    assert!(matches!(import(&site, &first), Outcome::Wrote));
    let id = first.id;

    let stale = ResourceRevision::new(7).expect("a revision");
    assert!(matches!(
        run(&site, move |view| unregister(view, id, stale)
            .expect("reads")),
        Outcome::Refused(EnvOpError::RevisionMismatch { .. })
    ));
    let missing = EnvironmentId::generate();
    assert!(matches!(
        run(&site, move |view| {
            unregister(view, missing, ResourceRevision::INITIAL).expect("reads")
        }),
        Outcome::Refused(EnvOpError::NotFound)
    ));
    assert!(matches!(
        run(&site, move |view| {
            unregister(view, id, ResourceRevision::INITIAL).expect("reads")
        }),
        Outcome::Wrote
    ));
    assert!(stored(&site).is_empty());
}

#[test]
fn manifest_location_resolves_the_parent_and_keeps_the_file_name() {
    let site = Site::new();
    let dir = site.root.path().join("real");
    std::fs::create_dir_all(&dir).expect("the folder can be made");
    std::os::unix::fs::symlink(&dir, site.root.path().join("alias")).expect("links");
    let fs = site.platform.fs();
    let through_link = stackly_core::environment::manifest_location(
        fs,
        &site.root.path().join("alias/stackly.yaml"),
    )
    .expect("resolves");
    let direct = stackly_core::environment::manifest_location(fs, &dir.join("stackly.yaml"))
        .expect("resolves");
    assert_eq!(through_link, direct);
    assert_eq!(
        direct,
        ManifestLocation::new(
            std::fs::canonicalize(&dir)
                .expect("canonical")
                .join("stackly.yaml")
        )
        .expect("absolute")
    );
}

// covers: AC-11 (spec 0044)
#[test]
fn manifest_location_spells_the_file_name_as_the_folder_stores_it() {
    let site = Site::new();
    let dir = site.root.path().join("proj");
    std::fs::create_dir_all(&dir).expect("the folder can be made");
    std::fs::write(dir.join("stackly.yaml"), "x").expect("the file writes");
    let fs = site.platform.fs();
    let stored = stackly_core::environment::manifest_location(fs, &dir.join("stackly.yaml"))
        .expect("resolves");
    let other_case = dir.join("Stackly.yaml");
    if std::fs::symlink_metadata(&other_case).is_err() {
        // A case sensitive volume: the two names are two files.
        return;
    }
    let upper = stackly_core::environment::manifest_location(fs, &other_case).expect("resolves");
    assert_eq!(upper, stored, "one file, one location");
    assert!(upper.as_str().ends_with("/stackly.yaml"));

    // A symlink in the folder is never followed to its target's name.
    std::os::unix::fs::symlink(dir.join("stackly.yaml"), dir.join("linked.yaml")).expect("links");
    let linked = stackly_core::environment::manifest_location(fs, &dir.join("linked.yaml"))
        .expect("resolves");
    assert!(linked.as_str().ends_with("/linked.yaml"));
}
