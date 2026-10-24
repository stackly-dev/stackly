import Foundation
import StacklyClient

/// One instance as a table row, an Overview row, and the inspector read it: the facts
/// already mapped to the words and types of the UI (spec 0022, *Halaman Instances*).
/// The provider's display name and tile come from the catalog entry whose `provider` equals
/// `InstanceSummary.provider.name`; without an entry the row shows the `software` as it is
/// and a gray tile.
nonisolated public struct InstanceRowModel: Identifiable, Equatable, Sendable {
    /// The detail a row shows when the last refetch of the instance failed.
    static let refreshFailedText = "Refresh failed"

    /// The instance ID.
    public let id: UUID
    /// The summary the row was made from.
    public let summary: InstanceSummary
    /// The display name of the provider, or the `software` without a catalog entry.
    public let providerTitle: String
    /// What the tile draws.
    public let tile: TileSpec
    /// The group, or `nil` for a retained instance.
    public let group: InstanceGroup?
    /// How the status looks, with the detail "Refresh failed" when the refetch failed.
    public let appearance: StatusAppearance
    /// The port text of the row ("`:5432`", "`auto`", "`socket`").
    public let ports: String
    /// Whether the last refetch of the instance failed.
    public let refreshFailed: Bool

    /// Builds the row of an instance.
    /// - Parameters:
    ///   - summary: The instance.
    ///   - provider: The catalog entry of its provider, or `nil` before the catalog loaded.
    ///   - refreshFailed: Whether the last refetch of this instance failed.
    public init(_ summary: InstanceSummary, provider: ProviderSummary?, refreshFailed: Bool) {
        id = summary.id
        self.summary = summary
        providerTitle = provider?.displayName ?? summary.software
        tile =
            provider.map {
                TileSpec(mark: $0.mark, tile: $0.tile, name: $0.displayName)
            } ?? TileSpec(mark: nil, tile: nil, name: summary.software)
        group = InstanceGroup.of(summary)
        let appearance = StatusAdapter.appearance(of: summary)
        self.appearance =
            refreshFailed
            ? StatusAppearance(
                label: appearance.label, tone: appearance.tone, symbol: appearance.symbol,
                detail: StatusDetail(text: Self.refreshFailedText, tone: .caution))
            : appearance
        ports = InstanceText.ports(summary.endpoints)
        self.refreshFailed = refreshFailed
    }

    /// The rows of `instances`, in the order given.
    /// - Parameters:
    ///   - instances: The instances.
    ///   - catalog: The catalog, or `nil` before it loaded.
    ///   - failures: The IDs whose last refetch failed.
    public static func rows(
        of instances: [InstanceSummary],
        catalog: [ProviderSummary]?,
        failures: Set<UUID> = []
    ) -> [InstanceRowModel] {
        let providers = Dictionary(
            (catalog ?? []).map { ($0.provider, $0) }, uniquingKeysWith: { first, _ in first })
        return instances.map {
            InstanceRowModel(
                $0, provider: providers[$0.provider.name], refreshFailed: failures.contains($0.id))
        }
    }

    // MARK: - Facts

    /// The instance name.
    public var name: String { summary.name }

    /// The exact runtime version, or `nil`.
    public var version: String? {
        summary.runtimeVersion.isEmpty ? nil : summary.runtimeVersion
    }

    /// "{provider} {version}", with "—" for a version that is not known.
    public var providerAndVersion: String {
        "\(providerTitle) \(version ?? "—")"
    }

    /// The meta line of an Overview row: "{provider} {version} · {ports}".
    public var meta: String {
        "\(providerAndVersion) · \(ports)"
    }

}
