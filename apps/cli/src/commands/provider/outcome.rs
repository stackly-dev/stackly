//! The result of `import`, `approve`, and `revoke`: the `ProviderDetail`
//! the daemon answered as JSON, and one line about what happened with the
//! entry's identity and trust in text.

use serde::Serialize;
use stackly_protocol::catalog::ProviderDetail;
use stackly_protocol::provider::ApprovalState;

use super::copy;
use super::local::esc;
use crate::output::human::{Cell, HumanWriter, Render};

/// One provider verb's outcome.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Outcome {
    #[serde(skip)]
    headline: String,
    detail: ProviderDetail,
}

impl Outcome {
    /// `detail` with the line that says what happened, written by the CLI
    /// with daemon text already escaped.
    #[must_use]
    pub const fn new(headline: String, detail: ProviderDetail) -> Self {
        Self { headline, detail }
    }
}

impl Render for Outcome {
    fn render(&self, out: &mut HumanWriter) {
        out.line(&self.headline);
        let summary = &self.detail.summary;
        let mut rows = vec![
            ("ID", Cell::text(&summary.provider)),
            (
                "Revision",
                Cell::optional(summary.provider_revision.as_deref()),
            ),
            ("Trust", Cell::fixed("local, not verified")),
            (
                "Approval",
                Cell::fixed(copy::approval_label(
                    summary.approval.unwrap_or(ApprovalState::Pending),
                )),
            ),
        ];
        if let Some(local) = &self.detail.local {
            rows.push((
                "Package",
                Cell::fixed(format!("sha256:{}", esc(&local.package_sha256))),
            ));
        }
        out.kv(&rows);
    }
}
