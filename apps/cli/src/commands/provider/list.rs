//! `stackly provider list` (spec 0017, AC-9): one line about the catalog,
//! then a table of its providers. `--category` filters on the client, so
//! the JSON `items` follow it too. Spec 0037 adds the `TRUST` column, the
//! approval of local entries in `STATE`, the count of imported entries,
//! and the revisions the installed catalog revokes.

use clap::{Args, ValueEnum};
use serde::Serialize;
use stackly_protocol::catalog::{
    CatalogInfo, CatalogSourceKind, ListProvidersReply, ProviderCategory, ProviderSummary,
    SupportReason, TrustLevel,
};
use stackly_protocol::provider::{ApprovalState, CatalogRevocation};
use stackly_protocol::{Call, Reply, Timestamp};

use super::local::esc;
use super::render::state_tone;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::Tone;
use crate::session::CallSpec;

/// The catalog is older than this many days when `outdated` is set.
const OUTDATED_DAYS: u32 = 180;

/// `--category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CategoryArg {
    /// Database servers.
    Database,
    /// Message brokers.
    Messaging,
    /// Analytics stores.
    Analytics,
    /// Caches.
    Cache,
    /// Proxies.
    Proxy,
    /// Test fixtures.
    Testing,
}

impl CategoryArg {
    /// The category on the wire.
    #[must_use]
    pub const fn category(self) -> ProviderCategory {
        match self {
            Self::Database => ProviderCategory::Database,
            Self::Messaging => ProviderCategory::Messaging,
            Self::Analytics => ProviderCategory::Analytics,
            Self::Cache => ProviderCategory::Cache,
            Self::Proxy => ProviderCategory::Proxy,
            Self::Testing => ProviderCategory::Testing,
        }
    }
}

/// `stackly provider list`.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only providers of one category; the catalog line is not filtered
    #[arg(long, value_enum, value_name = "CATEGORY")]
    pub category: Option<CategoryArg>,
}

/// The `provider list` result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderList {
    /// The facts about the catalog.
    pub catalog: CatalogInfo,
    /// The entries that pass `--category`, in the daemon's order.
    pub items: Vec<ProviderSummary>,
    /// The revisions the installed catalog revokes, not filtered.
    pub revocations: Vec<CatalogRevocation>,
    #[serde(skip)]
    category: Option<CategoryArg>,
}

impl RunCommand for ListArgs {
    type Output = ProviderList;

    async fn run(self, cx: &mut Context) -> CommandOutput<ProviderList> {
        let reply = match cx
            .call(Call::CatalogListProviders, CallSpec::control())
            .await
        {
            Ok(Reply::CatalogListProviders(reply)) => reply,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        CommandOutput::ok(ProviderList::of(reply, self.category))
    }
}

impl ProviderList {
    /// `reply` with the entries of `category` only, or all of them.
    fn of(reply: ListProvidersReply, category: Option<CategoryArg>) -> Self {
        let items = reply
            .items
            .into_iter()
            .filter(|item| category.is_none_or(|wanted| item.category == Some(wanted.category())))
            .collect();
        Self {
            catalog: reply.catalog,
            items,
            revocations: reply.revocations,
            category,
        }
    }
}

impl Render for ProviderList {
    fn render(&self, out: &mut HumanWriter) {
        out.line(&catalog_line(out, &self.catalog));
        if self.items.is_empty() {
            match self.category {
                Some(category) => out.line(&format!(
                    "No providers in category {}.",
                    wire_name(&category.category())
                )),
                None => out.line("No providers in the catalog."),
            }
        } else {
            let rows: Vec<Vec<Cell>> = self.items.iter().map(row).collect();
            out.table(
                &[
                    "provider", "name", "category", "state", "trust", "versions", "license",
                ],
                &rows,
            );
        }
        if !self.revocations.is_empty() {
            out.line("");
            out.line(&format!(
                "Revoked by this catalog (revision {}, published {}):",
                self.catalog.revision.get(),
                super::local::date(out, self.catalog.published_at)
            ));
            for revocation in &self.revocations {
                out.line(&revocation_line(revocation));
            }
        }
    }
}

/// `  <provider> <revision> since <date>: <reason>`, with the manifest
/// digest when the revocation names one.
fn revocation_line(revocation: &CatalogRevocation) -> String {
    let mut line = format!(
        "  {} {}",
        esc(&revocation.provider),
        esc(&revocation.provider_revision)
    );
    if let Some(digest) = &revocation.manifest_sha256 {
        line.push_str(&format!(" (manifest sha256:{})", esc(digest)));
    }
    line.push_str(&format!(
        " since {}: {}",
        esc(&revocation.since),
        esc(&revocation.reason)
    ));
    line
}

/// `Catalog: official, bundled, revision 4, published 2026-11-02 16:41
/// +07:00`, with `development` alone for a catalog folder from the
/// environment, and a note once the catalog is older than 180 days.
fn catalog_line(out: &HumanWriter, catalog: &CatalogInfo) -> String {
    let origin = if catalog.source == CatalogSourceKind::Development {
        "development".to_owned()
    } else {
        format!(
            "{}, {}",
            wire_name(&catalog.trust),
            wire_name(&catalog.source)
        )
    };
    let mut line = format!(
        "Catalog: {origin}, revision {}, published {}",
        catalog.revision.get(),
        minute(out, catalog.published_at)
    );
    let local = catalog.local_count.get();
    if local > 0 {
        line.push_str(&format!(", {local} imported"));
    }
    if catalog.outdated {
        line.push_str(&format!(
            ", older than {OUTDATED_DAYS} days; updating Stackly may bring a newer catalog"
        ));
    }
    line
}

/// `ts` in local time down to the minute: `2026-11-02 16:41 +07:00`.
fn minute(out: &HumanWriter, ts: Timestamp) -> String {
    let full = out.time(ts);
    // `YYYY-MM-DD HH:MM:SS ±HH:MM`: the seconds are the three characters
    // after the sixteenth.
    match (full.get(..16), full.get(19..)) {
        (Some(head), Some(tail)) => format!("{head}{tail}"),
        _ => full,
    }
}

/// One row of the table.
fn row(item: &ProviderSummary) -> Vec<Cell> {
    vec![
        Cell::text(&item.provider),
        Cell::text(&item.display_name),
        Cell::optional(item.category.as_ref().map(wire_name).as_deref()),
        state(item),
        Cell::text(&wire_name(&item.trust)),
        Cell::text(&versions(item)),
        Cell::text(&item.licenses.join(", ")),
    ]
}

/// The state: `needs approval` or `revoked` for a local entry that is not
/// approved or whose revision is revoked, else the state and its reason.
fn state(item: &ProviderSummary) -> Cell {
    if item.trust == TrustLevel::Local {
        let revoked = item.approval == Some(ApprovalState::Revoked)
            || item.state_reason == Some(SupportReason::TrustRevoked);
        if revoked {
            return Cell::toned("revoked", Tone::Caution);
        }
        if item.approval != Some(ApprovalState::Approved) {
            return Cell::toned("needs approval", Tone::Caution);
        }
    }
    let mut state = wire_name(&item.state);
    if let Some(reason) = &item.state_reason {
        state.push_str(&format!(" ({})", wire_name(reason)));
    }
    Cell::toned(&state, state_tone(item.state))
}

/// The versions with an artifact for this Mac, then `, +<n> more` for the
/// ones the summary leaves out.
fn versions(item: &ProviderSummary) -> String {
    let mut text = item.versions.join(", ");
    let more = item
        .version_count
        .get()
        .saturating_sub(item.versions.len() as u64);
    if more > 0 {
        text.push_str(&format!(", +{more} more"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(versions: &[&str], count: u64) -> ProviderSummary {
        serde_json::from_value(serde_json::json!({
            "provider": "official/x", "revision": 1, "displayName": "X",
            "state": "available", "trust": "official",
            "versions": versions, "versionCount": count,
            "licenses": [], "mark": "X", "tile": "gray",
        }))
        .expect("a summary decodes")
    }

    #[test]
    fn versions_beyond_the_sixteen_listed_are_counted() {
        assert_eq!(versions(&summary(&["1.0", "1.1"], 2)), "1.0, 1.1");
        assert_eq!(
            versions(&summary(&["1.0", "1.1"], 20)),
            "1.0, 1.1, +18 more"
        );
        assert_eq!(versions(&summary(&[], 0)), "");
    }

    #[test]
    fn a_missing_category_never_matches_a_filter() {
        let reply: ListProvidersReply = serde_json::from_value(serde_json::json!({
            "catalog": {
                "revision": 1, "publishedAt": "2026-10-20T01:26:00.000Z",
                "loadedAt": "2026-10-20T02:00:00.000Z",
                "contentSha256": "0".repeat(64), "source": "bundled",
                "trust": "official", "host": {"os": "macos", "arch": "aarch64"},
                "outdated": false, "invalidCount": 0,
            },
            "items": [summary(&[], 0)],
        }))
        .expect("a reply decodes");
        let list = ProviderList::of(reply, Some(CategoryArg::Database));
        assert!(list.items.is_empty());
    }
}
