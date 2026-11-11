import StacklyClient
import SwiftUI

/// The Import Provider sheet (spec 0037, *Layar*, AC-15): what the daemon found in the
/// folder before anything is imported. It shows the identity, the package rows, the
/// quarantine note, the permissions, how it relates to an earlier import, and the fixed
/// footnote; a package that can't be imported shows why, and a folder that can't be read
/// offers another one. Import sends the digest the sheet shows.
struct ImportProviderSheet: View {
    /// The most problems the banner of a package that can't be imported lists.
    static let problemLimit = 20

    let model: ProviderImportModel

    var body: some View {
        StacklySheet(
            title: ProviderTrustCopy.importTitle,
            subtitle: subtitle,
            tile: tile,
            primary: primary,
            cancel: StacklyAction(
                id: "catalog.import.cancel", title: "Cancel", mutates: false,
                perform: { model.cancel() }),
            minWidth: 520
        ) {
            ImportPreviewContent(phase: model.phase, chooseAnother: chooseAnother)
        }
    }

    private var subtitle: String? {
        guard let preview = model.preview else { return nil }
        return ProviderTrustCopy.previewSubtitle(
            provider: preview.provider, providerRevision: preview.providerRevision)
    }

    private var tile: TileSpec? {
        guard let preview = model.preview else { return nil }
        let name = preview.displayName ?? preview.provider
        return TileSpec(mark: ProviderMark.defaultMark(for: name), tile: .gray, name: name)
    }

    private var chooseAnother: @MainActor () -> Void {
        { Task { await model.begin() } }
    }

    /// Import, Import Again, or Preview Again, by the phase.
    private var primary: StacklyAction {
        switch model.phase {
        case .failed:
            return StacklyAction(
                id: "catalog.import.again", title: ProviderTrustCopy.previewAgainAction,
                mutates: false, perform: { Task { await model.previewAgain() } })
        case .preview(let preview, _), .importing(let preview, _):
            let isInvalidEntry =
                preview.existing.map {
                    $0.samePackage && $0.storedCopyValid == false
                } ?? false
            return StacklyAction(
                id: "catalog.import.confirm",
                title: isInvalidEntry
                    ? ProviderTrustCopy.importAgainAction : ProviderTrustCopy.importAction,
                mutates: true,
                disabledReason: Self.disabledReason(preview),
                isBusy: model.phase == .importing(preview, path: pathOf(model.phase)),
                perform: { Task { await model.importPackage() } })
        case .idle, .choosing, .loading, .unreadable:
            return StacklyAction(
                id: "catalog.import.confirm", title: ProviderTrustCopy.importAction,
                mutates: true, disabledReason: nil, isBusy: model.phase != .idle,
                perform: {})
        }
    }

    private func pathOf(_ phase: ProviderImportModel.Phase) -> String {
        switch phase {
        case .importing(_, let path): path
        default: ""
        }
    }

    /// Why Import can't run for `preview`, or `nil`. A package already imported with the
    /// same bytes needs nothing; the sheet already says so.
    static func disabledReason(_ preview: ImportPreview) -> String? {
        if ProviderImportModel.canImport(preview) { return nil }
        if preview.verdict.reason == .importLimit { return ProviderTrustCopy.importLimit }
        if preview.verdict.importable == false { return ProviderTrustCopy.invalidTitle + "." }
        return ProviderTrustCopy.samePackage
    }
}

/// The body of the Import Provider sheet for one phase.
struct ImportPreviewContent: View {
    let phase: ProviderImportModel.Phase
    let chooseAnother: @MainActor () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s16) {
            switch phase {
            case .idle, .choosing, .loading:
                HStack(spacing: Spacing.s8) {
                    DelayedProgress()
                    Text(ProviderTrustCopy.reading)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            case .unreadable(let error, _):
                StateBanner(
                    tone: .critical, title: ProviderTrustCopy.unreadableTitle,
                    message: [error.message, error.hint].compactMap { $0 }
                        .map(DaemonText.sentence).joined(separator: " "),
                    code: error.code,
                    actions: [
                        StateAction(
                            title: ProviderTrustCopy.chooseAnotherFolder, mutates: false,
                            perform: chooseAnother)
                    ])
            case .preview(let preview, _), .importing(let preview, _):
                ImportPreviewBody(preview: preview)
            case .failed(let preview, _, let error):
                StateBanner(
                    tone: .critical, title: ProviderTrustCopy.importFailedTitle,
                    message: [error.message, error.hint].compactMap { $0 }
                        .map(DaemonText.sentence).joined(separator: " "),
                    code: error.code)
                ImportPreviewBody(preview: preview)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// What the daemon found in the folder.
struct ImportPreviewBody: View {
    let preview: ImportPreview

    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s16) {
            if preview.verdict.importable == false {
                StateBanner(
                    tone: .critical, title: ProviderTrustCopy.invalidTitle,
                    message: preview.verdict.reason == .importLimit
                        ? ProviderTrustCopy.importLimit : nil,
                    items: Self.items(
                        Self.problems(of: preview), omitted: Int(preview.omittedFindings)))
            } else {
                let warnings = Self.warnings(of: preview)
                if warnings.isEmpty == false {
                    StateBanner(
                        tone: .caution, title: ProviderTrustCopy.warningsTitle(warnings.count),
                        items: Self.items(warnings))
                }
            }
            if let description = preview.description {
                Text(description)
                    .fixedSize(horizontal: false, vertical: true)
            }
            CardSection {
                LabeledContent(
                    "Publisher",
                    value: preview.provider.map { ProviderTrustCopy.publisher(provider: $0) }
                        ?? ProviderFacts.notListed)
                LabeledContent("Category", value: CatalogCopy.categoryName(preview.category))
                LabeledContent("Software", value: preview.software ?? ProviderFacts.notListed)
                LabeledContent("Versions", value: versions)
                LabeledContent(
                    "Package",
                    value: preview.packageSha256.map(ProviderTrustCopy.shortDigest)
                        ?? ProviderFacts.notListed)
                LabeledContent("Folder", value: preview.originPath)
            }
            .labeledContentStyle(.stackly)
            ForEach(Self.notes(of: preview), id: \.self) { note in
                Text(note)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let permissions = preview.permissions {
                CardSection("Permissions") {
                    ProviderPermissionsList(permissions: permissions)
                }
            }
            Text(ProviderTrustCopy.importFootnote)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var versions: String {
        let shown = preview.versions ?? []
        guard shown.isEmpty == false else { return ProviderFacts.notListed }
        let rest = Int(preview.versionCount) - shown.count
        let list = shown.joined(separator: ", ")
        return rest > 0 ? "\(list), \(ProviderTrustCopy.andMore(rest))" : list
    }

    /// The notes under the rows, in order: quarantine, the earlier import, the changes
    /// since the last decision, and the catalog revocation.
    static func notes(of preview: ImportPreview) -> [String] {
        var notes: [String] = []
        if preview.quarantined == true {
            notes.append(ProviderTrustCopy.quarantineNote)
        }
        if let existing = preview.existing {
            if existing.samePackage {
                notes.append(
                    existing.storedCopyValid
                        ? ProviderTrustCopy.samePackage : ProviderTrustCopy.damagedCopy)
            } else {
                notes.append(ProviderTrustCopy.replaces(revision: existing.providerRevision))
            }
        }
        if let changes = preview.changes, changes.isEmpty == false {
            notes.append(ProviderTrustCopy.importChanges(changes))
        }
        if preview.revocation != nil {
            notes.append(ProviderTrustCopy.importRevoked)
        }
        return notes
    }

    /// One problem line: the file and location, then the message.
    struct Problem: Equatable {
        let place: String
        let message: String
    }

    /// The error findings, then the import issues, as `file location: message` lines.
    static func problems(of preview: ImportPreview) -> [Problem] {
        let findings = (preview.findings ?? []).filter { $0.severity == .error }.map {
            Problem(place: "\($0.file) \($0.location)", message: $0.message)
        }
        let issues = (preview.importIssues ?? []).map {
            Problem(place: "\($0.file) \($0.location)", message: $0.message)
        }
        return findings + issues
    }

    /// The warning findings of an importable package.
    static func warnings(of preview: ImportPreview) -> [Problem] {
        (preview.findings ?? []).filter { $0.severity == .warning }.map {
            Problem(place: "\($0.file) \($0.location)", message: $0.message)
        }
    }

    /// Up to ``ImportProviderSheet/problemLimit`` problems as banner items, then "and N
    /// more" for the rest and for the findings the daemon left out, with names that never
    /// repeat.
    static func items(_ problems: [Problem], omitted: Int = 0) -> [PartialItem] {
        var used: Set<String> = []
        var items = problems.prefix(ImportProviderSheet.problemLimit).map { problem in
            var name = problem.place
            var suffix = 1
            while used.insert(name).inserted == false {
                suffix += 1
                name = "\(problem.place) (\(suffix))"
            }
            return PartialItem(name: name, reason: DaemonText.sentence(problem.message))
        }
        let rest = max(0, problems.count - ImportProviderSheet.problemLimit) + omitted
        if rest > 0 {
            items.append(PartialItem(name: ProviderTrustCopy.andMore(rest), reason: ""))
        }
        return items
    }
}
