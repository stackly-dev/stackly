import Foundation
import StacklyClient

/// Every word the GUI shows about importing, approving, revoking, and removing a local
/// provider (spec 0037, *Copy* and *Tone dan label*).
/// The daemon decides trust, approval, and revocation; this type only says them. No text
/// here claims a sandbox, a safe binary, a verified publisher, or a revocation list that is
/// current, and `ProviderTrustCopyTests` scans every string for those phrases. The approval
/// statement is the sentence of `protocols/fixtures/provider/approval-statement.txt`, the
/// file the CLI golden reads too.
nonisolated public enum ProviderTrustCopy {
    // MARK: - The approval statement

    /// The approval statement: what approving lets a provider do.
    public static let approvalStatement =
        "This provider runs native programs on your Mac with your user account’s permissions. Stackly does not sandbox them, so they can read and change the same files you can. A matching checksum only shows that a download was not altered. It does not show that the program is safe."

    /// The line under the statement in the approve sheet.
    public static let approveTrustNote = "Approve only if you trust where this provider came from."

    /// The footnote of the approve sheet.
    public static let approveFootnote =
        "This approval covers exactly this version of the provider. Any change needs a new approval."

    /// The footnote of the import sheet.
    public static let importFootnote =
        "Importing doesn’t run anything. The provider stays blocked until you approve it."

    /// The note of a folder that carries the quarantine attribute.
    public static let quarantineNote =
        "This folder was downloaded from the internet. Stackly only reads its text files and never changes the folder."

    // MARK: - Import sheet notes

    /// The note of a package whose bytes are the stored ones.
    public static let samePackage = "This exact package is already imported."

    /// The note of a package whose bytes are the stored ones while the stored copy is
    /// damaged.
    public static let damagedCopy =
        "Stackly’s copy of this package is damaged. Importing again repairs it and keeps your decision."

    /// The note of a package that replaces another imported revision.
    public static func replaces(revision: String) -> String {
        "Replaces imported revision \(revision). The new version needs approval before it can install or run anything."
    }

    /// The note of a package that differs from the one the person decided on.
    public static func importChanges(_ changes: [PermissionChange]) -> String {
        "Differs from the version you decided on: \(changeList(changes))."
    }

    /// The note of a package the installed catalog revokes.
    public static let importRevoked =
        "The catalog in this version of Stackly revokes this revision. You can import it to inspect it, but you can’t approve it."

    /// The note of a package past the import limit.
    public static let importLimit =
        "You’ve reached the limit for imported providers. Remove one first."

    /// The title of the banner of a package that can't be imported.
    public static let invalidTitle = "This package can’t be imported"

    /// The title of the banner of the validator's warnings.
    public static func warningsTitle(_ count: Int) -> String {
        count == 1 ? "1 warning from the validator" : "\(count) warnings from the validator"
    }

    /// The last line of a list that was cut: "and 3 more".
    public static func andMore(_ count: Int) -> String {
        "and \(count) more"
    }

    // MARK: - Titles and buttons

    /// The toolbar and Settings button.
    public static let importButton = "Import Provider…"
    /// The title of the import sheet.
    public static let importTitle = "Import Provider"
    /// The primary button of the import sheet.
    public static let importAction = "Import"
    /// The primary button of the import sheet for an entry whose stored copy is invalid.
    public static let importAgainAction = "Import Again"
    /// The button of a folder that can't be read.
    public static let chooseAnotherFolder = "Choose Another Folder…"
    /// The primary button of the import sheet after the import failed: it reads the folder
    /// again.
    public static let previewAgainAction = "Preview Again"
    /// The message of the folder panel.
    public static let folderPanelMessage = "Choose the folder that contains provider.yaml."
    /// The help of the import button while stacklyd is offline.
    public static let offlineHelp = "Not available while stacklyd is offline."
    /// The title of a folder that can't be read.
    public static let unreadableTitle = "Can’t Read This Folder"
    /// The title of an import that failed.
    public static let importFailedTitle = "Couldn’t Import This Provider"
    /// The line while the daemon reads the folder.
    public static let reading = "Reading the provider package…"

    /// The button that opens the approve sheet.
    public static let approveButton = "Approve…"
    /// The button that opens the approve sheet again after a revoke.
    public static let approveAgainButton = "Approve Again…"
    /// The primary button of the approve sheet.
    public static let approveAction = "Approve"
    /// The button that asks to revoke.
    public static let revokeButton = "Revoke Approval…"
    /// The button that asks to remove.
    public static let removeButton = "Remove…"
    /// The title of an approval that failed.
    public static let approveFailedTitle = "Couldn’t Approve This Provider"

    /// The title of the approve sheet.
    public static func approveTitle(displayName: String) -> String {
        "Approve “\(displayName)”?"
    }

    /// The identity line of the approve sheet: "{id} · revision {providerRevision} · from
    /// {namespace} (not verified)".
    public static func identityLine(provider: String, providerRevision: String?) -> String {
        var parts = [provider]
        if let providerRevision {
            parts.append("revision \(providerRevision)")
        }
        parts.append("from \(namespace(of: provider)) (not verified)")
        return parts.joined(separator: " · ")
    }

    /// The subtitle of the import sheet header: "{id} · revision {providerRevision}".
    public static func previewSubtitle(provider: String?, providerRevision: String?) -> String? {
        let parts = [provider, providerRevision.map { "revision \($0)" }].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    // MARK: - Revoke and remove

    /// The title of the revoke dialog.
    public static func revokeTitle(displayName: String) -> String {
        "Revoke approval for “\(displayName)”?"
    }

    /// The message of the revoke dialog.
    public static let revokeMessage =
        "Stackly will stop installing runtimes and creating instances from this provider, including from revisions you approved earlier. Existing instances, their data, and installed runtimes stay as they are, and existing instances can still start."

    /// The action of the revoke dialog.
    public static let revokeAction = "Revoke Approval"

    /// The title of the remove dialog.
    public static func removeTitle(displayName: String) -> String {
        "Remove “\(displayName)” from the catalog?"
    }

    /// The message of the remove dialog.
    public static let removeMessage =
        "Removing also revokes its approval. Instances created from it, their data, and installed runtimes are not touched, and those instances can still start. To create new instances from it, import and approve it again. The folder you imported from is not touched."

    /// The action of the remove dialog.
    public static let removeAction = "Remove"

    // MARK: - Feedback

    /// The feedback after an import.
    public static func imported(displayName: String) -> String {
        "Imported \(displayName). Approve it before you install anything."
    }

    /// The feedback after an approval.
    public static func approved(displayName: String) -> String {
        "Approved \(displayName)."
    }

    /// The feedback after a revoke.
    public static func revoked(displayName: String) -> String {
        "Revoked the approval of \(displayName)."
    }

    /// The feedback after a remove.
    public static func removed(displayName: String) -> String {
        "Removed \(displayName) from the catalog."
    }

    // MARK: - Labels

    /// The label of an approval state.
    public static func approvalLabel(_ approval: ApprovalState) -> String {
        switch approval {
        case .pending: "Needs Approval"
        case .approved: "Approved"
        case .revoked: "Revoked"
        case .unrecognized: "Unrecognized State"
        }
    }

    /// The label of what an executable is used for.
    public static func useLabel(_ use: ExecutableUse) -> String {
        switch use {
        case .start: "starts the server"
        case .initialize: "runs during setup"
        case .stop: "runs on stop"
        case .reload: "runs on reload"
        case .probe: "runs health checks"
        case .versionCheck: "runs once at install"
        case .openWith: "opens in Terminal"
        case .unrecognized: "Unknown"
        }
    }

    /// The label of one kind of change since a decision.
    public static func changeLabel(_ change: PermissionChange) -> String {
        switch change {
        case .revision: "revision"
        case .executables: "programs it runs"
        case .downloads: "files it downloads"
        case .downloadHosts: "download hosts"
        case .capabilities: "features"
        case .filesystem: "folders it writes"
        case .secrets: "secrets it receives"
        case .endpoints: "endpoints"
        case .openWith: "apps it opens"
        case .content: "configuration content"
        case .unrecognized: "Unknown"
        }
    }

    /// The labels of `changes`, joined with commas.
    public static func changeList(_ changes: [PermissionChange]) -> String {
        changes.map(changeLabel).joined(separator: ", ")
    }

    /// The label of a folder of an instance a provider writes to.
    public static func folderLabel(_ code: String) -> String {
        switch code {
        case "config": "Configuration"
        case "data": "Data"
        case "logs": "Logs"
        case "run": "Runtime state"
        case "temp": "Temporary files"
        case "cache": "Cache"
        case "backups": "Backups"
        default: code
        }
    }

    /// The label of the kind of an Open With action.
    public static func openWithLabel(_ action: PermissionOpenWith) -> String {
        switch action.kind {
        case "terminal": "\(action.name) in Terminal"
        case "app": action.target.map { "\(action.name) in \($0)" } ?? action.name
        case "url": "\(action.name) in the browser"
        default: action.name
        }
    }

    /// The text of an endpoint: "client · TCP port 7400".
    public static func endpointText(_ endpoint: PermissionEndpoint) -> String {
        var text = "\(endpoint.name) · \(endpoint.transport.uppercased())"
        if let port = endpoint.defaultPort {
            text += " port \(port)"
        }
        return text
    }

    /// The line of one download host: "downloads.example.test · 2 files".
    public static func hostLine(_ host: String, files: Int) -> String {
        files == 1 ? "\(host) · 1 file" : "\(host) · \(files) files"
    }

    /// The groups of the permissions list, in their fixed order.
    public enum PermissionGroup: String, CaseIterable, Sendable {
        /// The programs the provider runs.
        case runs = "Runs"
        /// What it downloads.
        case downloads = "Downloads"
        /// The folders it writes.
        case writes = "Writes to"
        /// The secrets it receives.
        case secrets = "Receives secrets"
        /// The endpoints it listens on.
        case endpoints = "Listens on"
        /// The apps it opens.
        case opens = "Opens"
        /// The features it has.
        case features = "Features"
    }

    // MARK: - Detail rows

    /// The namespace of a provider ID: "acme" for "acme/demo".
    public static func namespace(of provider: String) -> String {
        provider.split(separator: "/", maxSplits: 1).first.map(String.init) ?? provider
    }

    /// The Publisher row of a local entry.
    public static func publisher(provider: String) -> String {
        "\(namespace(of: provider)) (not verified)"
    }

    /// The Source row of a local entry: "Imported Nov 11, 2026 from ~/providers/acme-demo".
    public static func source(importedAt: Date, originPath: String, style: Date.FormatStyle)
        -> String
    {
        "Imported \(importedAt.formatted(style)) from \(originPath)"
    }

    /// The first 12 characters of a digest.
    public static func shortDigest(_ digest: String) -> String {
        String(digest.prefix(12))
    }

    // MARK: - Banner of a local entry

    /// The approval banner of a local entry (spec 0037, *Banner approval*).
    public struct Banner: Equatable, Sendable {
        /// What the person can do from the banner.
        public enum Action: Equatable, Sendable {
            /// Approve….
            case approve
            /// Approve Again….
            case approveAgain
        }

        /// The tone.
        public let tone: Tone
        /// The title.
        public let title: String
        /// The message.
        public let message: String
        /// The action, or `nil`.
        public let action: Action?
    }

    /// The banner of a local entry, or `nil` for an approved one and for an invalid one
    /// (which keeps the banner of spec 0017).
    /// - Parameters:
    ///   - detail: The detail the daemon sent.
    ///   - catalog: The catalog info of the last list, for the revision and the date the
    ///     catalog was published.
    ///   - style: How dates are written.
    public static func banner(
        detail: ProviderDetail, catalog: CatalogInfo?, style: Date.FormatStyle
    ) -> Banner? {
        guard detail.summary.trust == .local, detail.summary.state != .invalid else { return nil }
        if let revocation = detail.revocation, revocation.by == .catalog {
            return Banner(
                tone: .caution, title: "Revoked by the catalog",
                message: catalogRevokedMessage(
                    revocation, catalog: catalog, style: style),
                action: nil)
        }
        let decision = detail.local?.decision
        switch detail.summary.approval {
        case .approved:
            return nil
        case .revoked:
            let since = detail.revocation?.since ?? decision?.decidedAt
            let date = since.map { "Revoked on \($0.formatted(style)). " } ?? ""
            return Banner(
                tone: .caution, title: "Approval revoked",
                message: date
                    + "Stackly won’t install runtimes or create instances from this provider. Existing instances and their data are untouched and can still start.",
                action: .approveAgain)
        case .pending, .unrecognized, nil:
            guard let decision else {
                return Banner(
                    tone: .caution, title: "Not approved yet",
                    message:
                        "Stackly won’t install or run anything from this provider until you approve it.",
                    action: .approve)
            }
            let title =
                decision.decision == .revoked
                ? "Changed since you revoked it" : "Changed since your approval"
            let changes = detail.local?.changes ?? []
            let list = changes.isEmpty ? "" : ": \(changeList(changes))"
            return Banner(
                tone: .caution, title: title,
                message:
                    "This version differs from revision \(decision.providerRevision) that you decided on \(decision.decidedAt.formatted(style))\(list).",
                action: .approve)
        }
    }

    private static func catalogRevokedMessage(
        _ revocation: RevocationInfo, catalog: CatalogInfo?, style: Date.FormatStyle
    ) -> String {
        var source = "The catalog in this version of Stackly"
        var facts: [String] = []
        if let revision = revocation.catalogRevision ?? catalog?.revision {
            facts.append("revision \(revision)")
        }
        if let catalog {
            facts.append("published \(catalog.publishedAt.formatted(style))")
        }
        if facts.isEmpty == false {
            source += " (\(facts.joined(separator: ", ")))"
        }
        let since = revocation.since.formatted(style)
        let reason = revocation.reason.map { ": \(DaemonText.sentence($0))" } ?? "."
        return
            "\(source) revokes this revision since \(since)\(reason) Existing instances and their data are untouched and can still start."
    }

    // MARK: - Settings › Sources

    /// The title of the group of imported providers.
    public static let settingsGroup = "Imported providers"
    /// The group without any imported provider.
    public static let settingsEmpty = "No imported providers."
    /// The button that opens the main window on the entry.
    public static let showInCatalog = "Show in Catalog"

    /// The line under the name of an imported provider: "acme/demo · Needs Approval ·
    /// imported Nov 11, 2026".
    public static func settingsLine(
        provider: String, approval: ApprovalState?, importedAt: Date?, style: Date.FormatStyle
    ) -> String {
        var parts = [provider]
        if let approval {
            parts.append(approvalLabel(approval))
        }
        if let importedAt {
            parts.append("imported \(importedAt.formatted(style))")
        }
        return parts.joined(separator: " · ")
    }

    /// The line of the Catalog group when the catalog revokes revisions.
    public static func revokedRevisions(_ count: Int) -> String {
        count == 1
            ? "1 revoked revision in this catalog" : "\(count) revoked revisions in this catalog"
    }

    // MARK: - Every string, for the copy rules

    /// Every fixed string and one sample of every built string, for the tests that scan the
    /// copy of this feature for phrases it must never say.
    static var allSamples: [String] {
        var all = [
            approvalStatement, approveTrustNote, approveFootnote, importFootnote,
            quarantineNote, samePackage, damagedCopy, replaces(revision: "1.0.0"),
            importChanges(permissionChanges), importRevoked, importLimit, invalidTitle,
            warningsTitle(2), andMore(3), importButton, importTitle, importAction,
            importAgainAction, chooseAnotherFolder, previewAgainAction, folderPanelMessage,
            offlineHelp, unreadableTitle, importFailedTitle, reading, approveButton,
            approveAgainButton, approveAction, revokeButton, removeButton, approveFailedTitle,
            approveTitle(displayName: "Acme Demo"),
            identityLine(provider: "acme/demo", providerRevision: "1.0.0"),
            revokeTitle(displayName: "Acme Demo"), revokeMessage, revokeAction,
            removeTitle(displayName: "Acme Demo"), removeMessage, removeAction,
            imported(displayName: "Acme Demo"), approved(displayName: "Acme Demo"),
            revoked(displayName: "Acme Demo"), removed(displayName: "Acme Demo"),
            publisher(provider: "acme/demo"), settingsGroup, settingsEmpty, showInCatalog,
            revokedRevisions(2),
        ]
        all += approvalStates.map(approvalLabel)
        all += executableUses.map(useLabel)
        all += PermissionGroup.allCases.map(\.rawValue)
        return all
    }

    /// Every approval state, `unrecognized` last.
    static let approvalStates: [ApprovalState] = [.pending, .approved, .revoked, .unrecognized]

    /// Every executable use, in the order of spec 0037, `unrecognized` last.
    static let executableUses: [ExecutableUse] = [
        .start, .initialize, .stop, .reload, .probe, .versionCheck, .openWith, .unrecognized,
    ]

    /// Every change code, in the order of spec 0037, `unrecognized` last.
    static let permissionChanges: [PermissionChange] = [
        .revision, .executables, .downloads, .downloadHosts, .capabilities, .filesystem,
        .secrets, .endpoints, .openWith, .content, .unrecognized,
    ]
}
