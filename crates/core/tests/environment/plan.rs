//! `environment::plan`: AC-5, AC-7, AC-8, AC-9, AC-10.

use stackly_core::environment::{
    BlockerKind, Plan, PlanAction, PlanSecretStatus, PlanStep, Registration,
};
use stackly_domain::{InstanceLifecycle, Ownership, Registry, SecretRef, WriteBatch, WriteOp};
use stackly_platform::Clock;
use stackly_testkit::EnvironmentBuilder;

use crate::common::fixture;
use crate::site::Site;

fn codes(plan: &Plan) -> Vec<&'static str> {
    plan.blockers.iter().map(|b| b.kind.code()).collect()
}

fn count(plan: &Plan, step: PlanStep) -> usize {
    plan.items
        .iter()
        .flat_map(|item| &item.steps)
        .filter(|s| **s == step)
        .count()
}

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
";

// covers: AC-5, AC-10, AC-25 (spec 0044)
#[test]
fn the_erp_example_plans_six_installs_creates_and_starts_on_an_empty_root() {
    let site = Site::new();
    let plan = site.plan(&fixture("erp-development.yaml"));

    assert_eq!(codes(&plan), Vec::<&str>::new(), "{:#?}", plan.blockers);
    assert!(plan.applicable);
    assert_eq!(plan.registration, Some(Registration::New));
    assert_eq!(count(&plan, PlanStep::InstallRuntime), 6);
    assert_eq!(count(&plan, PlanStep::CreateInstance), 6);
    assert_eq!(count(&plan, PlanStep::StartInstance), 6);
    assert!(
        plan.items
            .iter()
            .all(|item| item.action == PlanAction::Create)
    );
    let postgres = &plan.items[0];
    assert_eq!(postgres.instance_name.as_str(), "erp-development-postgres");
    let runtime = postgres.runtime.as_ref().expect("a resolved runtime");
    assert_eq!(runtime.selector, "18");
    assert!(runtime.version.to_string().starts_with("18."));
    assert!(runtime.install && runtime.download_size.is_some());
    let row = postgres
        .checks
        .iter()
        .find(|c| c.id == "runtime")
        .expect("a runtime row");
    assert_eq!(row.detail, "will be installed");
    // Five unregistered references; traefik has no secret field.
    assert_eq!(plan.needs_secrets, 5);
    assert_eq!(postgres.secrets[0].status, PlanSecretStatus::Unregistered);
    assert!(plan.digest.is_some());
    let order: Vec<&str> = plan.order.iter().map(|a| a.as_str()).collect();
    assert_eq!(
        order,
        [
            "postgres",
            "mysql",
            "nats",
            "clickhouse",
            "valkey",
            "traefik"
        ]
    );
    let endpoints: Vec<String> = plan.items[2]
        .endpoints
        .iter()
        .map(|e| format!("{}={:?}", e.name, e.port))
        .collect();
    assert_eq!(
        endpoints.len(),
        2,
        "every IP endpoint of nats: {endpoints:?}"
    );
}

// covers: AC-5 (spec 0044)
#[test]
fn the_start_order_follows_dependencies_with_ties_in_manifest_order() {
    let site = Site::new();
    let plan = site.plan(&fixture("graph.yaml"));
    let order: Vec<&str> = plan.order.iter().map(|a| a.as_str()).collect();
    assert_eq!(order, ["db", "cache", "api", "web"]);
    let api = plan
        .items
        .iter()
        .find(|i| i.alias.as_str() == "api")
        .expect("api");
    let waits: Vec<&str> = api.waits_for.iter().map(|a| a.as_str()).collect();
    assert_eq!(waits, ["db", "cache"]);
    // The fake runtime is installed, so nothing is installed.
    assert_eq!(
        count(&plan, PlanStep::InstallRuntime),
        0,
        "{:#?}",
        plan.blockers
    );
    assert!(plan.applicable, "{:#?}", plan.blockers);
}

// covers: AC-6 (spec 0044)
#[test]
fn a_plan_writes_nothing_and_touches_no_secret_or_process() {
    let site = Site::new();
    let before = site
        .registry
        .known_instance_ids()
        .expect("reads")
        .generation;
    let files = tree(site.root.path());
    let _ = site.plan(&fixture("erp-development.yaml"));
    let _ = site.plan(&fixture("graph.yaml"));
    assert_eq!(
        site.registry
            .known_instance_ids()
            .expect("reads")
            .generation,
        before
    );
    assert_eq!(tree(site.root.path()), files);
    assert!(site.fake.processes().calls().is_empty());
    assert_eq!(
        site.fake.secrets().count(stackly_testkit::SecretOp::Write),
        0
    );
    assert!(site.fake.fetcher().requests().is_empty());
}

fn tree(root: &std::path::Path) -> Vec<(std::path::PathBuf, u64, std::time::SystemTime)> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("reads") {
            let entry = entry.expect("an entry");
            let meta = entry.metadata().expect("metadata");
            if meta.is_dir() {
                stack.push(entry.path());
            }
            found.push((entry.path(), meta.len(), meta.modified().expect("mtime")));
        }
    }
    found.sort();
    found
}

// covers: AC-7 (spec 0044)
#[test]
fn a_fixed_port_of_another_instance_and_a_taken_name_block_the_plan() {
    let mut site = Site::new();
    site.instance("other", 18080, InstanceLifecycle::Stopped);
    site.instance("shop-api", 18090, InstanceLifecycle::Stopped);
    let plan = site.plan(SHOP.as_bytes());
    let found = codes(&plan);
    assert!(found.contains(&"supervisor.endpoint_conflict"), "{found:?}");
    assert!(found.contains(&"instance.name_taken"), "{found:?}");
    assert!(!plan.applicable);
    assert!(plan.digest.is_none());
    assert_eq!(
        plan.blockers[0].service.as_ref().map(|a| a.as_str()),
        Some("api")
    );
}

// covers: AC-7 (spec 0044)
#[test]
fn a_port_in_use_and_an_unresolved_selector_block_the_plan() {
    let site = Site::new();
    site.probe.take(18080);
    let text = SHOP.replace("0.1", "9.9");
    let plan = site.plan(text.as_bytes());
    let found = codes(&plan);
    assert!(found.contains(&"supervisor.endpoint_in_use"), "{found:?}");
    assert!(found.contains(&"runtime.selector_unresolved"), "{found:?}");
    assert!(plan.items[0].runtime.is_none());
}

// covers: AC-7, AC-11 (spec 0044)
#[test]
fn a_name_registered_at_another_location_is_name_taken() {
    let site = Site::new();
    let env = EnvironmentBuilder::new()
        .name("shop")
        .manifest_location("/Users/example/elsewhere/stackly.yaml")
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            env.clone(),
        )]))
        .expect("commits");
    let plan = site.plan(SHOP.as_bytes());
    assert_eq!(plan.registration, Some(Registration::NameTaken));
    assert!(matches!(
        &plan.blockers[0].kind,
        BlockerKind::NameTaken { .. }
    ));
    assert_eq!(plan.environment.as_ref().map(|e| e.id), Some(env.id()));
}

// covers: AC-11 (spec 0044)
#[test]
fn a_registered_source_whose_file_declares_another_name_stops_with_manifest_renamed() {
    let site = Site::new();
    let location = site.location("stackly.yaml");
    let env = EnvironmentBuilder::new()
        .name("store")
        .manifest_location(location.as_str())
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            env.clone(),
        )]))
        .expect("commits");
    let plan = site.plan_at(SHOP.as_bytes(), &location, Some(env.id()));
    assert_eq!(codes(&plan), ["env.manifest_renamed"]);
    assert!(plan.items.is_empty());
}

const BORROW: &str = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  cache:
    provider: testing/fake
    instanceRef: shared-fake
";

// covers: AC-8 (spec 0044)
#[test]
fn an_instance_ref_is_kept_or_started_and_never_created() {
    let mut site = Site::new();
    assert_eq!(
        codes(&site.plan(BORROW.as_bytes())),
        ["env.instance_ref_not_found"]
    );

    site.instance("shared-fake", 18085, InstanceLifecycle::Running);
    let plan = site.plan(BORROW.as_bytes());
    assert_eq!(plan.items[0].action, PlanAction::Keep);
    assert_eq!(plan.items[0].ownership, Ownership::Shared);
    assert!(plan.applicable);

    site.views[0].lifecycle = InstanceLifecycle::Stopped;
    let plan = site.plan(BORROW.as_bytes());
    assert_eq!(plan.items[0].action, PlanAction::Start);
    assert_eq!(plan.items[0].steps, [PlanStep::StartInstance]);

    let other = BORROW.replace("testing/fake", "official/valkey");
    assert!(codes(&site.plan(other.as_bytes())).contains(&"env.instance_ref_mismatch"));
}

// covers: AC-8, AC-9 (spec 0044)
#[test]
fn an_instance_ref_never_names_a_retained_instance() {
    let mut site = Site::new();
    site.instance("shared-fake", 18085, InstanceLifecycle::Stopped);
    site.views[0].lifecycle = InstanceLifecycle::Retained;
    assert_eq!(
        codes(&site.plan(BORROW.as_bytes())),
        ["env.instance_ref_not_found"],
        "a retained instance gave its name up"
    );

    // A new instance took the name the retained one gave up.
    let mut live = site.views[0].clone();
    live.id = stackly_domain::InstanceId::generate();
    live.lifecycle = InstanceLifecycle::Running;
    site.views.push(live.clone());
    let plan = site.plan(BORROW.as_bytes());
    assert!(plan.applicable, "{:#?}", plan.blockers);
    assert_eq!(plan.items[0].instance_id, Some(live.id));
    assert_eq!(plan.items[0].action, PlanAction::Keep);
}

// covers: AC-9 (spec 0044)
#[test]
fn a_retained_member_keeps_its_item_and_blocks_the_plan() {
    let mut site = Site::new();
    registered(&mut site, InstanceLifecycle::Stopped);
    site.views[0].lifecycle = InstanceLifecycle::Retained;
    let plan = site.plan(MEMBERS.as_bytes());
    assert!(!plan.applicable);
    assert!(plan.digest.is_none());
    let api = plan
        .items
        .iter()
        .find(|item| item.alias.as_str() == "api")
        .expect("the retained member keeps its item");
    assert_eq!(api.action, PlanAction::Keep);
    assert!(api.steps.is_empty());
    let blocked: Vec<(&str, &str)> = plan
        .blockers
        .iter()
        .filter_map(|b| match &b.kind {
            BlockerKind::InstanceBlocked { instance, reason } => {
                Some((instance.as_str(), reason.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(blocked, [("shop-api", "retained")]);
}

const MEMBERS: &str = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  api:
    provider: testing/fake
    version: \"0.2\"
    endpoints:
      http: {port: 18081}
  web:
    provider: testing/fake
    version: \"0.1\"
";

/// Registers `shop` at the site's location with `api` as a dedicated
/// member and `old` as a member the manifest no longer has.
fn registered(site: &mut Site, lifecycle: InstanceLifecycle) -> stackly_domain::Environment {
    let api = site.instance("shop-api", 18080, lifecycle);
    let old = site.instance("shop-old", 18070, InstanceLifecycle::Stopped);
    let env = EnvironmentBuilder::new()
        .name("shop")
        .manifest_location(site.location("stackly.yaml").as_str())
        .member("api", api.id(), Ownership::Dedicated)
        .member("old", old.id(), Ownership::Dedicated)
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            env.clone(),
        )]))
        .expect("commits");
    env
}

// covers: AC-9 (spec 0044)
#[test]
fn a_member_follows_the_lifecycle_table_reports_differences_and_detached() {
    let mut site = Site::new();
    registered(&mut site, InstanceLifecycle::Running);
    let plan = site.plan(MEMBERS.as_bytes());
    assert_eq!(plan.registration, Some(Registration::Registered));
    let api = &plan.items[0];
    assert_eq!(api.action, PlanAction::Keep);
    let fields: Vec<&str> = api.differences.iter().map(|d| d.field.as_str()).collect();
    assert_eq!(fields, ["runtime", "endpoint.http.port"]);
    assert_eq!(plan.items[1].action, PlanAction::Create);
    assert_eq!(plan.detached.len(), 1);
    assert_eq!(plan.detached[0].alias.as_str(), "old");
    assert!(
        plan.applicable,
        "differences never block: {:#?}",
        plan.blockers
    );

    let table = [
        (InstanceLifecycle::Stopped, PlanAction::Start, None),
        (InstanceLifecycle::Failed, PlanAction::Start, None),
        (InstanceLifecycle::Stopping, PlanAction::Start, None),
        (InstanceLifecycle::Starting, PlanAction::Keep, None),
        (InstanceLifecycle::Initializing, PlanAction::Keep, None),
        (InstanceLifecycle::Recovering, PlanAction::Keep, None),
        (
            InstanceLifecycle::Unknown,
            PlanAction::Keep,
            Some("unknown"),
        ),
        (
            InstanceLifecycle::Retained,
            PlanAction::Keep,
            Some("retained"),
        ),
        (
            InstanceLifecycle::Uninitialized,
            PlanAction::Keep,
            Some("not_initialized"),
        ),
    ];
    for (lifecycle, action, blocked) in table {
        site.views[0].lifecycle = lifecycle;
        let plan = site.plan(MEMBERS.as_bytes());
        assert_eq!(plan.items[0].action, action, "{lifecycle:?}");
        let reason = plan.blockers.iter().find_map(|b| match &b.kind {
            BlockerKind::InstanceBlocked { reason, .. } => Some(reason.as_str()),
            _ => None,
        });
        assert_eq!(reason, blocked, "{lifecycle:?}");
    }

    site.views[0].lifecycle = InstanceLifecycle::Stopped;
    site.views[0].initialized = false;
    let plan = site.plan(MEMBERS.as_bytes());
    assert_eq!(
        plan.items[0].action,
        PlanAction::Keep,
        "stopped without data"
    );
    assert_eq!(codes(&plan), ["env.instance_blocked"]);

    site.views[0].initialized = true;
    site.views[0].blocker = Some("configDrift".to_owned());
    let plan = site.plan(MEMBERS.as_bytes());
    assert_eq!(
        codes(&plan),
        ["env.instance_blocked"],
        "a start that is refused"
    );
}

// covers: AC-7, AC-9 (spec 0044)
#[test]
fn a_new_service_on_the_fixed_port_of_a_member_is_blocked() {
    let mut site = Site::new();
    registered(&mut site, InstanceLifecycle::Running);
    let text = MEMBERS.replace("  web:\n    provider: testing/fake\n    version: \"0.1\"\n", "  web:\n    provider: testing/fake\n    version: \"0.1\"\n    endpoints:\n      http: {port: 18080}\n");
    let plan = site.plan(text.as_bytes());
    assert!(
        codes(&plan).contains(&"supervisor.endpoint_conflict"),
        "{:?}",
        codes(&plan)
    );
}

// covers: AC-5 (spec 0044)
#[test]
fn the_digest_ignores_member_lifecycle_and_ports_but_follows_what_up_creates() {
    let mut site = Site::new();
    registered(&mut site, InstanceLifecycle::Stopped);
    let first = site.plan(MEMBERS.as_bytes()).digest.expect("applicable");
    site.views[0].lifecycle = InstanceLifecycle::Running;
    assert_eq!(site.plan(MEMBERS.as_bytes()).digest, Some(first));
    site.views[0].endpoints[0].2 = stackly_domain::Port::new(18099).ok();
    assert_eq!(site.plan(MEMBERS.as_bytes()).digest, Some(first));

    let changed = MEMBERS.replace("  web:\n    provider: testing/fake\n    version: \"0.1\"\n", "  web:\n    provider: testing/fake\n    version: \"0.1\"\n    endpoints:\n      http: {port: 18095}\n");
    assert_ne!(
        site.plan(changed.as_bytes()).digest,
        Some(first),
        "a create endpoint"
    );

    let other = site.instance("shop-api-2", 18060, InstanceLifecycle::Running);
    let env = site.registry.environments().expect("reads").value.remove(0);
    let mut stored = stackly_testkit::EnvironmentBuilder::new()
        .with_id(env.id())
        .name("shop")
        .manifest_location(site.location("stackly.yaml").as_str())
        .member("api", other.id(), Ownership::Dedicated)
        .revision(2)
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateEnvironment {
            environment: stored.clone(),
            expected: env.revision(),
        }]))
        .expect("commits");
    let _ = &mut stored;
    assert_ne!(
        site.plan(MEMBERS.as_bytes()).digest,
        Some(first),
        "another member instance"
    );
}

// covers: AC-10 (spec 0044)
#[test]
fn bound_references_show_present_missing_unregistered_and_generated() {
    let site = Site::new();
    let text = "\
apiVersion: stackly/v1alpha1
kind: Environment
metadata:
  name: shop
services:
  present:
    provider: testing/fake
    version: \"0.1\"
    secretRefs: {authToken: \"local:shop/present\"}
  missing:
    provider: testing/fake
    version: \"0.1\"
    secretRefs: {authToken: \"local:shop/missing\"}
  unregistered:
    provider: testing/fake
    version: \"0.1\"
    secretRefs: {authToken: \"local:shop/unregistered\"}
  generated:
    provider: testing/fake
    version: \"0.1\"
";
    for name in ["present", "missing"] {
        let reference = SecretRef::parse(&format!("local:shop/{name}")).expect("a reference");
        let record = stackly_domain::SecretReferenceRecord::new(
            stackly_domain::SecretReferenceId::generate(),
            reference.clone(),
            stackly_domain::PlatformStoreKey::new(format!("fake:{name}")).expect("a key"),
            stackly_domain::SecretOwner::User,
            stackly_domain::SecretPurpose::user(),
            site.fake.clock().now(),
        );
        site.registry
            .commit(WriteBatch::new(vec![WriteOp::InsertSecretReference(
                record,
            )]))
            .expect("commits");
        if name == "present" {
            site.fake
                .secrets()
                .insert(&reference, "value-from-the-store");
        }
    }
    let plan = site.plan(text.as_bytes());
    let statuses: Vec<PlanSecretStatus> = plan
        .items
        .iter()
        .map(|item| item.secrets[0].status)
        .collect();
    assert_eq!(
        statuses,
        [
            PlanSecretStatus::Present,
            PlanSecretStatus::Missing,
            PlanSecretStatus::Unregistered,
            PlanSecretStatus::Generated,
        ]
    );
    assert_eq!(plan.needs_secrets, 2);
    assert!(plan.applicable, "{:#?}", plan.blockers);
    let shown = format!("{plan:?}");
    assert!(!shown.contains("value-from-the-store"));
}

// covers: AC-2 (spec 0044)
#[test]
fn an_invalid_manifest_is_never_applicable() {
    let site = Site::new();
    let plan = site.plan(&fixture("invalid/dependency_cycle.yaml"));
    assert!(!plan.applicable);
    assert!(plan.items.is_empty() && plan.registration.is_none());
}
