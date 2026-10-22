//! `runtime.list` and `runtime.get` on an empty registry and on bad
//! params (spec 0018 AC-18); the runtimes an install leaves are read in
//! the install tests.

use stackly_protocol::codes::resource;
use stackly_protocol::runtime::{GetRuntimeParams, ListRuntimesParams};
use stackly_protocol::{Call, Count, Reply, ResourceKind};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::support::{Site, catalog, client};

#[tokio::test]
async fn an_empty_registry_lists_nothing_and_an_unknown_runtime_is_not_found() {
    let ev = evidence!(
        covers = ["AC-018"],
        kind = Kind::Fixture,
        expected = "runtime.list answers an empty page, runtime.get names the missing runtime, and a limit outside 1 to 200 is refused",
    );
    let site = Site::new();
    let _daemon = site.start(catalog(Vec::new())).await;
    let client = client(&site).await;

    let Reply::RuntimeList(page) = client
        .call(Call::RuntimeList(ListRuntimesParams::default()))
        .await
        .expect("an empty registry lists")
    else {
        panic!("expected a runtime.list reply");
    };
    assert!(page.items.is_empty() && page.installing.is_empty() && page.next.is_none());

    let id = Uuid::now_v7();
    let missing = client
        .call(Call::RuntimeGet(GetRuntimeParams { runtime_id: id }))
        .await
        .unwrap_err();
    assert_eq!(missing.code(), resource::NOT_FOUND.code);
    assert_eq!(
        missing
            .resource()
            .map(|resource| (resource.kind, resource.id.clone())),
        Some((ResourceKind::Runtime, id.to_string()))
    );

    for limit in [0, 201] {
        let error = client
            .call(Call::RuntimeList(ListRuntimesParams {
                limit: Count::new(limit),
                after: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "request.invalid_params", "{limit}");
    }
    ev.pass();
}
