//! The words of `stackly provider import`, `approve`, `revoke`, and
//! `remove` (spec 0037, *Copy*): one module, so the copy rules (AC-11) can
//! be checked in one place. The approval statement is the line of
//! `protocols/fixtures/provider/approval-statement.txt`, which the Swift
//! app shows too; a test compares the two.

use stackly_protocol::provider::{ApprovalState, ExecutableUse, PermissionChange};

/// The approval statement, printed exactly before `approve` asks.
pub const APPROVAL_STATEMENT: &str = "This provider runs native programs on your Mac with your user account’s permissions. Stackly does not sandbox them, so they can read and change the same files you can. A matching checksum only shows that a download was not altered. It does not show that the program is safe.";

/// The line after the approval statement.
pub const APPROVE_FOOTNOTE: &str =
    "This approval covers exactly this version of the provider. Any change needs a new approval.";

/// The last line of an import preview.
pub const IMPORT_FOOTNOTE: &str =
    "Importing doesn’t run anything. The provider stays blocked until you approve it.";

/// The note of a folder with `com.apple.quarantine`.
pub const QUARANTINE_NOTE: &str = "This folder was downloaded from the internet. Stackly only reads its text files and never changes the folder.";

/// The note of a preview whose exact bytes are already imported.
pub const SAME_PACKAGE: &str = "This exact package is already imported.";

/// The note of a preview whose bytes are imported but whose stored copy
/// does not load.
pub const DAMAGED_COPY: &str = "Stackly’s copy of this package is damaged. Importing again repairs it and keeps your decision.";

/// The note of a preview of a revision the installed catalog revokes.
pub const REVOKED_BY_CATALOG: &str = "The catalog in this version of Stackly revokes this revision. You can import it to inspect it, but you can’t approve it.";

/// The note of a preview that the import limit blocks.
pub const IMPORT_LIMIT: &str = "You’ve reached the limit for imported providers. Remove one first.";

/// The line a repeated import of the same bytes writes.
pub const ALREADY_IMPORTED: &str = "Already imported.";

/// What revoking does, shown before `revoke` asks.
pub const REVOKE_CONSEQUENCE: &str = "Stackly will stop installing runtimes and creating instances from this provider, including from revisions you approved earlier. Existing instances, their data, and installed runtimes stay as they are, and existing instances can still start.";

/// What removing does, shown before `remove` asks.
pub const REMOVE_CONSEQUENCE: &str = "Removing also revokes its approval. Instances created from it, their data, and installed runtimes are not touched, and those instances can still start. To create new instances from it, import and approve it again. The folder you imported from is not touched.";

/// The note of a preview that replaces another imported revision.
#[must_use]
pub fn replaces(revision: &str) -> String {
    format!(
        "Replaces imported revision {revision}. The new version needs approval before it can install or run anything."
    )
}

/// The note of a preview that differs from the last decision.
#[must_use]
pub fn differs(changes: &[PermissionChange]) -> String {
    format!(
        "Differs from the version you decided on: {}.",
        change_labels(changes)
    )
}

/// The question of `approve`.
#[must_use]
pub fn approve_question(provider: &str) -> String {
    format!("Approve {provider}?")
}

/// The question of `revoke`.
#[must_use]
pub fn revoke_question(display_name: &str) -> String {
    format!("Revoke approval for “{display_name}”?")
}

/// The question of `remove`.
#[must_use]
pub fn remove_question(display_name: &str) -> String {
    format!("Remove “{display_name}” from the catalog?")
}

/// The identity line above the approval statement.
#[must_use]
pub fn identity_line(provider: &str, revision: &str, publisher: &str) -> String {
    format!("{provider} · revision {revision} · from {publisher} (not verified)")
}

/// The label of what an executable is used for.
#[must_use]
pub const fn use_label(used: ExecutableUse) -> &'static str {
    match used {
        ExecutableUse::Start => "starts the server",
        ExecutableUse::Initialize => "runs during setup",
        ExecutableUse::Stop => "runs on stop",
        ExecutableUse::Reload => "runs on reload",
        ExecutableUse::Probe => "runs health checks",
        ExecutableUse::VersionCheck => "runs once at install",
        ExecutableUse::OpenWith => "opens in Terminal",
        ExecutableUse::Unrecognized => "unknown",
    }
}

/// The label of one kind of change since the last decision.
#[must_use]
pub const fn change_label(change: PermissionChange) -> &'static str {
    match change {
        PermissionChange::Revision => "revision",
        PermissionChange::Executables => "programs it runs",
        PermissionChange::Downloads => "files it downloads",
        PermissionChange::DownloadHosts => "download hosts",
        PermissionChange::Capabilities => "features",
        PermissionChange::Filesystem => "folders it writes",
        PermissionChange::Secrets => "secrets it receives",
        PermissionChange::Endpoints => "endpoints",
        PermissionChange::OpenWith => "apps it opens",
        PermissionChange::Content => "configuration content",
        PermissionChange::Unrecognized => "unknown",
    }
}

/// The labels of `changes`, joined with commas.
#[must_use]
pub fn change_labels(changes: &[PermissionChange]) -> String {
    changes
        .iter()
        .map(|change| change_label(*change))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The words of an approval state in the CLI.
#[must_use]
pub const fn approval_label(state: ApprovalState) -> &'static str {
    match state {
        ApprovalState::Pending => "needs approval",
        ApprovalState::Approved => "approved",
        ApprovalState::Revoked => "revoked",
        ApprovalState::Unrecognized => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/provider")
            .join(name);
        std::fs::read_to_string(&path).expect("the fixture reads")
    }

    /// Every fixed sentence and template of this module.
    fn all_copy() -> Vec<String> {
        let mut texts: Vec<String> = [
            APPROVAL_STATEMENT,
            APPROVE_FOOTNOTE,
            IMPORT_FOOTNOTE,
            QUARANTINE_NOTE,
            SAME_PACKAGE,
            DAMAGED_COPY,
            REVOKED_BY_CATALOG,
            IMPORT_LIMIT,
            ALREADY_IMPORTED,
            REVOKE_CONSEQUENCE,
            REMOVE_CONSEQUENCE,
        ]
        .iter()
        .map(|text| (*text).to_owned())
        .collect();
        texts.push(replaces("1.0.0"));
        texts.push(differs(&[PermissionChange::Revision]));
        texts.push(approve_question("acme/demo"));
        texts.push(revoke_question("Acme Demo"));
        texts.push(remove_question("Acme Demo"));
        texts.push(identity_line("acme/demo", "1.0.0", "acme"));
        texts
    }

    #[test]
    fn the_approval_statement_is_the_fixture_line_exactly() {
        assert_eq!(
            fixture("approval-statement.txt").trim_end_matches('\n'),
            APPROVAL_STATEMENT
        );
    }

    #[test]
    fn no_copy_claims_safety_verification_or_freshness() {
        let forbidden = [
            "sandboxed",
            "safe to run",
            "verified publisher",
            "trusted publisher",
            "malware",
            "up to date",
            "latest",
        ];
        for text in all_copy() {
            let lower = text.to_lowercase();
            for phrase in forbidden {
                assert!(!lower.contains(phrase), "{phrase:?} in {text:?}");
            }
        }
    }

    #[test]
    fn changes_are_joined_in_their_order_with_their_labels() {
        assert_eq!(
            differs(&[PermissionChange::Revision, PermissionChange::Downloads]),
            "Differs from the version you decided on: revision, files it downloads."
        );
        assert_eq!(change_label(PermissionChange::Unrecognized), "unknown");
        assert_eq!(
            use_label(ExecutableUse::VersionCheck),
            "runs once at install"
        );
    }
}
