//! The catalog DTOs the scripted daemon answers with: the wire fixtures of
//! `protocols/fixtures/catalog/`, the same files the Swift view model tests
//! read, and the shapes they lack made from the same DTOs.

use std::path::PathBuf;

use stackly_protocol::catalog::{
    CatalogSourceKind, ListProvidersReply, ProviderDetail, TrustLevel,
};
use stackly_protocol::{Call, Reply};

use crate::common::scripted::{Ctx, Step, error};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures/catalog")
}

fn read(name: &str) -> String {
    let path = fixture_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} reads: {error}", path.display()))
}

/// The reply of `catalog.list_providers`: ten entries, two of them invalid.
pub fn list() -> ListProvidersReply {
    serde_json::from_str(&read("list-providers.json")).expect("the list fixture decodes")
}

/// The detail of the fixture entry `name`, such as `nats`.
pub fn detail(name: &str) -> ProviderDetail {
    serde_json::from_str(&read(&format!("provider-{name}.json"))).expect("a detail fixture decodes")
}

/// The list of a development catalog that is older than 180 days.
pub fn list_development_outdated() -> ListProvidersReply {
    let mut reply = list();
    reply.catalog.source = CatalogSourceKind::Development;
    reply.catalog.trust = TrustLevel::Development;
    reply.catalog.outdated = true;
    reply
}

/// The list with no entries.
pub fn list_empty() -> ListProvidersReply {
    let mut reply = list();
    reply.items.clear();
    reply.catalog.invalid_count = stackly_protocol::Count::saturating(0);
    reply
}

/// A scripted daemon handler: `reply` for the list, and for `get_provider`
/// the fixture whose summary names the provider, else
/// `catalog.provider_not_found`.
pub fn catalog(
    reply: ListProvidersReply,
    details: Vec<ProviderDetail>,
) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static {
    move |call, _| match call {
        Call::CatalogListProviders => {
            vec![Step::Reply(Reply::CatalogListProviders(reply.clone()))]
        }
        Call::CatalogGetProvider(params) => {
            match details
                .iter()
                .find(|detail| detail.summary.provider == params.provider)
            {
                Some(detail) => vec![Step::Reply(Reply::CatalogGetProvider(Box::new(
                    detail.clone(),
                )))],
                None => vec![Step::Fail(error(
                    "catalog.provider_not_found",
                    "not_found",
                    &format!("no provider {} in the catalog", params.provider),
                ))],
            }
        }
        _ => Vec::new(),
    }
}

/// Every detail fixture.
pub fn all_details() -> Vec<ProviderDetail> {
    [
        "nats",
        "intelonly",
        "blockedhost",
        "postgresql",
        "broken",
        "zenith",
    ]
    .iter()
    .map(|name| detail(name))
    .collect()
}
