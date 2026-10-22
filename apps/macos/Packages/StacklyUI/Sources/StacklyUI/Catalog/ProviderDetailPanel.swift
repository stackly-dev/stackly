import StacklyClient
import SwiftUI

/// The content of the inspector on the Catalog screen (spec 0017, *Layar Catalog (GUI)*):
/// the header, the description, and, by the provider's state, the versions, the
/// capabilities, and the key value rows.
///
/// The header, the description, and the reason come from the card's summary, so they show
/// at once; the rest waits for `catalog.get_provider`. A planned provider has no matrix,
/// no capability, and no button, only its note. An invalid one shows why it failed. The
/// panel has no Get, New Instance, or Import Provider button: those belong to features 18,
/// 20, and 37.
struct ProviderDetailPanel: View {
    /// The most issues an invalid provider lists.
    static let issueLimit = 5

    let summary: ProviderSummary
    let phase: ProviderDetailModel.Phase
    let host: HostTarget
    let retry: @MainActor () -> Void

    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s16) {
            InspectorHeader(
                title: summary.displayName,
                subtitle: "\(summary.provider) · \(CatalogCopy.categoryName(summary.category))",
                tile: TileSpec(
                    mark: summary.mark, tile: summary.state == .planned ? .gray : summary.tile,
                    name: summary.displayName),
                status: CatalogAppearance.appearance(of: summary.state))
            if let description = summary.description {
                Text(description)
                    .font(.body)
                    .foregroundStyle(
                        summary.state == .planned
                            ? AnyShapeStyle(StacklyColor.textSecondary) : AnyShapeStyle(.primary)
                    )
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let reason = stateReason {
                Text(reason)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            switch phase {
            case .loading:
                ProgressView()
                    .controlSize(.small)
                    .frame(maxWidth: .infinity)
                    .accessibilityLabel("Loading \(summary.displayName)")
            case .failed(let failure):
                StateBanner(
                    tone: .critical, title: failure.title, message: failure.message,
                    code: failure.code,
                    actions: [StateAction(title: "Retry", mutates: false, perform: retry)])
            case .loaded(let detail):
                loaded(detail)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityIdentifier("catalog.detail")
    }

    /// The reason line under the description of an unsupported or blocked provider.
    private var stateReason: String? {
        switch summary.state {
        case .unsupported, .blocked:
            summary.stateReason.map {
                CatalogCopy.reasonText($0, note: summary.stateNote)
            }
        case .available, .planned, .invalid, .unrecognized:
            nil
        }
    }

    @ViewBuilder
    private func loaded(_ detail: ProviderDetail) -> some View {
        switch summary.state {
        case .planned:
            Text(detail.note ?? CatalogCopy.plannedNote)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        case .invalid:
            invalid(detail)
        case .available, .blocked, .unsupported, .unrecognized:
            ready(detail)
        }
    }

    private func invalid(_ detail: ProviderDetail) -> some View {
        let issues = detail.issues ?? []
        return StateBanner(
            tone: .critical, title: "This provider failed validation", code: "manifest.invalid",
            items: Self.issueItems(issues))
    }

    /// The first issues as banner items, named so that two issues never share a name.
    static func issueItems(_ issues: [CatalogIssue]) -> [PartialItem] {
        var used: Set<String> = []
        return issues.prefix(issueLimit).map { issue in
            var name = "\(issue.file) · \(issue.location)"
            var suffix = 1
            while used.insert(name).inserted == false {
                suffix += 1
                name = "\(issue.file) · \(issue.location) (\(suffix))"
            }
            return PartialItem(name: name, reason: issue.message)
        }
    }

    @ViewBuilder
    private func ready(_ detail: ProviderDetail) -> some View {
        let versions = detail.versions ?? []
        let notes = VersionMatrix.notes(of: versions)
        let facts = ProviderFacts(detail: detail, host: host, locale: locale)
        if versions.isEmpty == false {
            CardSection(
                "Versions",
                accessory: { EmptyView() },
                footer: {
                    VStack(alignment: .leading, spacing: Spacing.s4) {
                        ForEach(notes, id: \.self) { Text($0) }
                    }
                },
                content: { VersionMatrix(versions: versions) })
        }
        CardSection("Capabilities") {
            CapabilityList(capabilities: detail.capabilities ?? [])
        }
        .accessibilityIdentifier(AccessibilityID.Catalog.capabilities)
        CardSection {
            LabeledContent("License") {
                licenseValue(facts)
            }
            LabeledContent("Source", value: list(facts.sources))
            LabeledContent("Download", value: facts.download)
            LabeledContent("Disk", value: facts.disk)
            LabeledContent("Requires", value: facts.requires)
            LabeledContent("Dependencies", value: facts.dependencies)
            LabeledContent("Publisher", value: facts.publisher)
            LabeledContent("Revision", value: facts.revision)
            LabeledContent("Trust", value: facts.trust)
        }
        .labeledContentStyle(.stackly)
    }

    @ViewBuilder
    private func licenseValue(_ facts: ProviderFacts) -> some View {
        if let url = facts.licenseURL, let license = facts.licenses.first {
            Link(license, destination: url)
        } else {
            Text(list(facts.licenses))
        }
    }

    private func list(_ values: [String]) -> String {
        values.isEmpty ? ProviderFacts.notListed : values.joined(separator: ", ")
    }
}
