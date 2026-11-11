//! The DTOs of spec 0037 the scripted daemon answers with: the wire
//! fixtures of `protocols/fixtures/provider/`, the same files the Swift
//! tests read, and the shapes they lack made from the same DTOs.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use stackly_protocol::catalog::{ListProvidersReply, ProviderDetail};
use stackly_protocol::provider::{ImportPreview, RemovedProvider};
use stackly_protocol::{Call, ErrorBody, Reply};

use crate::common::scripted::{Ctx, Step, error};
use crate::data;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures/provider")
}

/// The text of the provider fixture `name`.
pub fn read(name: &str) -> String {
    let path = fixture_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {} reads: {error}", path.display()))
}

/// The preview fixture `import-preview-<name>.json`.
pub fn preview(name: &str) -> ImportPreview {
    serde_json::from_str(&read(&format!("import-preview-{name}.json")))
        .expect("a preview fixture decodes")
}

/// The detail fixture `detail-local-<name>.json`.
pub fn local(name: &str) -> ProviderDetail {
    serde_json::from_str(&read(&format!("detail-local-{name}.json")))
        .expect("a detail fixture decodes")
}

/// The pending preview of bytes that are already imported.
pub fn preview_same() -> ImportPreview {
    let mut preview = preview("pending");
    preview.existing = Some(
        serde_json::from_value(serde_json::json!({
            "providerRevision": "1.0.0",
            "packageSha256": "f".repeat(64),
            "approval": "approved",
            "samePackage": true,
            "storedCopyValid": true,
        }))
        .expect("an existing import decodes"),
    );
    preview
}

/// A valid package that the import limit blocks.
pub fn preview_limit() -> ImportPreview {
    let mut preview = preview("pending");
    preview.verdict = serde_json::from_value(serde_json::json!({
        "importable": false, "reason": "import_limit",
    }))
    .expect("a verdict decodes");
    preview
}

/// The pending entry after a new import replaced an approved one: the
/// decision is for other bytes, with what changed.
pub fn local_changed() -> ProviderDetail {
    let mut detail = local("pending");
    let local = detail.local.as_mut().expect("a local entry");
    local.decision = Some(
        serde_json::from_value(serde_json::json!({
            "decision": "approved",
            "decidedAt": "2026-11-10T08:00:00.000Z",
            "providerRevision": "0.9.0",
            "packageSha256": "a".repeat(64),
        }))
        .expect("a decision decodes"),
    );
    local.changes = serde_json::from_value(serde_json::json!(["revision", "downloads"]))
        .expect("changes decode");
    detail
}

/// A local entry whose stored copy does not load.
pub fn local_invalid() -> ProviderDetail {
    let mut detail = local("pending");
    detail.summary.state =
        serde_json::from_value(serde_json::json!("invalid")).expect("a state decodes");
    detail.summary.state_reason = None;
    detail.versions.clear();
    detail.capabilities.clear();
    detail.manifest_sha256 = None;
    detail.issues = serde_json::from_value(serde_json::json!([{
        "file": "provider.yaml", "location": "metadata.id",
        "code": "import_digest_mismatch",
        "message": "the stored copy does not match the import record",
        "reason": null,
    }]))
    .expect("issues decode");
    let local = detail.local.as_mut().expect("a local entry");
    local.permissions = None;
    detail
}

/// The catalog list with three local entries (pending, approved,
/// revoked), the local count, and one catalog revocation.
pub fn list_with_local() -> ListProvidersReply {
    let mut reply = data::list();
    for (name, id) in [
        ("pending", "acme/demo"),
        ("approved", "acme/approved"),
        ("revoked", "acme/revoked"),
    ] {
        let mut summary = local(name).summary;
        summary.provider = id.to_owned();
        reply.items.push(summary);
    }
    reply.catalog.local_count = stackly_protocol::Count::saturating(3);
    reply.revocations = serde_json::from_value(serde_json::json!([{
        "provider": "official/nats",
        "providerRevision": "0.9.0",
        "manifestSha256": null,
        "reason": "Starts with an insecure default; use revision 1.0.0 or later.",
        "since": "2026-10-01",
    }]))
    .expect("revocations decode");
    reply
}

/// What the scripted provider daemon answers, changed by tests between
/// runs.
#[derive(Debug, Clone)]
pub struct Script {
    /// The preview for every path.
    pub preview: ImportPreview,
    /// The detail `catalog.get_provider` answers for an `acme/` ID.
    pub detail: ProviderDetail,
    /// The reply of `provider.import`.
    pub imported: ProviderDetail,
    /// The error the mutation methods answer instead, if any.
    pub fail: Option<ErrorBody>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            preview: preview("pending"),
            detail: local("pending"),
            imported: local("pending"),
            fail: None,
        }
    }
}

/// A scripted daemon handler over `script`: the catalog fixtures for
/// `official/` entries, the script for previews, imports, and `acme/`
/// details, and the approved, revoked, and removed fixtures for the
/// decisions.
pub fn provider(script: Arc<Mutex<Script>>) -> impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync {
    let catalog = data::catalog(list_with_local(), data::all_details());
    move |call, ctx| {
        let script = script.lock().expect("the script lock").clone();
        let mutation = matches!(
            call,
            Call::ProviderImport(_)
                | Call::ProviderApprove(_)
                | Call::ProviderRevoke(_)
                | Call::ProviderRemove(_)
        );
        if mutation && let Some(body) = script.fail {
            return vec![Step::Fail(body)];
        }
        match call {
            Call::ProviderPreviewImport(_) => vec![Step::Reply(Reply::ProviderPreviewImport(
                Box::new(script.preview),
            ))],
            Call::ProviderImport(_) => {
                vec![Step::Reply(Reply::ProviderImport(Box::new(
                    script.imported,
                )))]
            }
            Call::CatalogGetProvider(params) if params.provider.starts_with("acme/") => {
                vec![Step::Reply(Reply::CatalogGetProvider(Box::new(
                    script.detail,
                )))]
            }
            Call::ProviderApprove(_) => vec![Step::Reply(Reply::ProviderApprove(Box::new(local(
                "approved",
            ))))],
            Call::ProviderRevoke(_) => vec![Step::Reply(Reply::ProviderRevoke(Box::new(local(
                "revoked",
            ))))],
            Call::ProviderRemove(params) => {
                vec![Step::Reply(Reply::ProviderRemove(RemovedProvider {
                    provider: params.provider.clone(),
                }))]
            }
            other => catalog(other, ctx),
        }
    }
}

/// An error body as the daemon sends it.
pub fn daemon_error(code: &str, category: &str) -> ErrorBody {
    error(code, category, "scripted failure")
}
