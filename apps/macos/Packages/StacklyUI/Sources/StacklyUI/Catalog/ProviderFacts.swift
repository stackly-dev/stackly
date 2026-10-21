import Foundation
import StacklyClient

/// The key value rows of the detail panel (spec 0017, *Layar Catalog (GUI)*): License,
/// Source, Download, Disk, Requires, Dependencies, Publisher, Revision, and Trust.
///
/// License, Source, Download, Disk, Requires, and Dependencies read only the artifacts for
/// this Mac, the ones whose OS and architecture equal the catalog's host. That filters
/// what is shown; it never changes a status, which the daemon computes.
nonisolated struct ProviderFacts: Equatable {
    /// The text of a row the catalog has no value for.
    static let notListed = "Not listed"

    /// The unique licenses of the artifacts for this Mac, in order of appearance.
    let licenses: [String]
    /// The license link, only when the artifacts for this Mac name one license and one
    /// web address for it.
    let licenseURL: URL?
    /// The unique sources, such as `github.com/nats-io`.
    let sources: [String]
    /// "6.8 MB", or "6.8 MB to 48.1 MB" when the versions differ.
    let download: String
    /// The unpacked size, written like ``download``.
    let disk: String
    /// "macOS 13.0 or later on Apple Silicon".
    let requires: String
    /// The dependencies row.
    let dependencies: String
    /// The publisher.
    let publisher: String
    /// The provider revision and the first 12 characters of the manifest digest.
    let revision: String
    /// The trust row.
    let trust: String

    /// Reads the rows from a provider detail.
    ///
    /// - Parameters:
    ///   - detail: The detail the daemon sent.
    ///   - host: This Mac, from the catalog info.
    ///   - locale: The locale of the sizes.
    init(detail: ProviderDetail, host: HostTarget, locale: Locale = .current) {
        let artifacts = (detail.versions ?? [])
            .flatMap { $0.artifacts ?? [] }
            .filter { $0.os == host.os && $0.arch == host.arch }
        licenses = Self.unique(artifacts.compactMap(\.license))
        let urls = Self.unique(artifacts.compactMap(\.licenseUrl))
        licenseURL =
            licenses.count == 1 && urls.count == 1 ? Self.webURL(urls[0]) : nil
        sources = Self.unique(artifacts.compactMap(\.source))
        download = Self.range(of: artifacts.compactMap(\.downloadSize), locale: locale)
        disk = Self.range(of: artifacts.compactMap(\.installedSize), locale: locale)
        requires = Self.requires(artifacts.compactMap(\.minOs), host: host)
        dependencies = Self.dependencies(artifacts)
        publisher = detail.summary.publisher ?? Self.notListed
        revision = Self.revision(
            detail.summary.providerRevision, manifestSha256: detail.manifestSha256)
        trust = CatalogCopy.trustText(detail.summary.trust)
    }

    private static func unique(_ values: [String]) -> [String] {
        var seen: Set<String> = []
        return values.filter { seen.insert($0).inserted }
    }

    private static func webURL(_ text: String) -> URL? {
        guard let url = URL(string: text), ["http", "https"].contains(url.scheme?.lowercased())
        else { return nil }
        return url
    }

    private static func range(of sizes: [UInt64], locale: Locale) -> String {
        guard let low = sizes.min(), let high = sizes.max() else { return notListed }
        let first = CatalogCopy.sizeText(low, locale: locale)
        let last = CatalogCopy.sizeText(high, locale: locale)
        return first == last ? first : "\(first) to \(last)"
    }

    private static func requires(_ versions: [String], host: HostTarget) -> String {
        let lowest = versions.min { lhs, rhs in
            lhs.compare(rhs, options: .numeric) == .orderedAscending
        }
        guard let lowest else { return notListed }
        let os = CatalogCopy.osName(host.os)
        return "\(os) \(lowest) or later on \(CatalogCopy.archName(host.arch, on: host.os))"
    }

    private static func dependencies(_ artifacts: [CatalogArtifact]) -> String {
        let lists = artifacts.compactMap(\.dependencies)
        guard lists.isEmpty == false else { return "None declared" }
        let bundled = unique(
            lists.flatMap { $0 }.filter { $0.kind == .bundled }.map { dependency in
                var text = dependency.name
                if let version = dependency.version { text += " \(version)" }
                if let license = dependency.license { text += " (\(license))" }
                return text
            })
        return bundled.isEmpty ? "None" : bundled.joined(separator: ", ")
    }

    private static func revision(_ revision: String?, manifestSha256: String?) -> String {
        let digest = manifestSha256.map { String($0.prefix(12)) }
        switch (revision, digest) {
        case (let revision?, let digest?): return "\(revision) · \(digest)"
        case (let revision?, nil): return revision
        case (nil, let digest?): return digest
        case (nil, nil): return notListed
        }
    }
}
