import StacklyClient
import SwiftUI

/// What a local provider may do once approved (spec 0037, *Layar*): fixed groups in a
/// fixed order (Runs, Downloads, Writes to, Receives secrets, Listens on, Opens, Features),
/// each left out when it is empty. The daemon computes the permissions from the package;
/// the list only says them.
struct ProviderPermissionsList: View {
    let permissions: ProviderPermissions

    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s12) {
            ForEach(Self.groups(of: permissions), id: \.self) { group in
                VStack(alignment: .leading, spacing: Spacing.s6) {
                    Text(group.rawValue)
                        .font(.headline)
                    content(of: group)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// The groups that have something to show, in their fixed order.
    static func groups(of permissions: ProviderPermissions)
        -> [ProviderTrustCopy.PermissionGroup]
    {
        ProviderTrustCopy.PermissionGroup.allCases.filter { group in
            switch group {
            case .runs: permissions.executables?.isEmpty == false
            case .downloads: permissions.downloads?.isEmpty == false
            case .writes: permissions.managedFilesystem?.isEmpty == false
            case .secrets: permissions.secrets?.isEmpty == false
            case .endpoints: permissions.endpoints?.isEmpty == false
            case .opens: permissions.openWith?.isEmpty == false
            case .features: permissions.capabilities?.isEmpty == false
            }
        }
    }

    /// The download hosts in the order they first appear, with how many files each serves.
    static func hosts(of downloads: [PermissionDownload]) -> [(host: String, files: Int)] {
        var order: [String] = []
        var counts: [String: Int] = [:]
        for download in downloads {
            for host in download.hosts ?? [] {
                if counts[host] == nil { order.append(host) }
                counts[host, default: 0] += 1
            }
        }
        return order.map { ($0, counts[$0] ?? 0) }
    }

    @ViewBuilder private func content(of group: ProviderTrustCopy.PermissionGroup) -> some View {
        switch group {
        case .runs:
            ForEach(Array((permissions.executables ?? []).enumerated()), id: \.offset) {
                _, executable in
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(executable.path)
                        .font(.body.monospaced())
                        .textSelection(.enabled)
                    let uses = (executable.uses ?? []).map(ProviderTrustCopy.useLabel)
                    if uses.isEmpty == false {
                        secondary(uses.joined(separator: ", "))
                    }
                }
            }
        case .downloads:
            downloads
        case .writes:
            lines((permissions.managedFilesystem ?? []).map(ProviderTrustCopy.folderLabel))
        case .secrets:
            lines(permissions.secrets ?? [])
        case .endpoints:
            lines((permissions.endpoints ?? []).map(ProviderTrustCopy.endpointText))
        case .opens:
            lines((permissions.openWith ?? []).map(ProviderTrustCopy.openWithLabel))
        case .features:
            lines((permissions.capabilities ?? []).map(CatalogCopy.capabilityLabel))
        }
    }

    @ViewBuilder private var downloads: some View {
        let all = permissions.downloads ?? []
        ForEach(Self.hosts(of: all), id: \.host) { entry in
            Text(ProviderTrustCopy.hostLine(entry.host, files: entry.files))
        }
        DisclosureGroup("Files") {
            VStack(alignment: .leading, spacing: Spacing.s4) {
                ForEach(Array(all.enumerated()), id: \.offset) { _, download in
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Text("\(download.artifact) · \(download.version)")
                        secondary(
                            [
                                "\(CatalogCopy.osName(download.os)) \(CatalogCopy.archName(download.arch, on: download.os))",
                                ProviderTrustCopy.shortDigest(download.sha256),
                                CatalogCopy.sizeText(download.size, locale: locale),
                            ].joined(separator: " · "))
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.top, Spacing.s4)
        }
    }

    private func lines(_ values: [String]) -> some View {
        ForEach(Array(values.enumerated()), id: \.offset) { _, value in
            Text(value)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func secondary(_ text: String) -> some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(StacklyColor.textSecondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}
