import Foundation
import StacklyClient

struct RuntimeRowModel: Identifiable, Equatable {
    let summary: RuntimeSummary
    let tile: TileSpec

    var id: UUID { summary.id }
    /// "PostgreSQL 17.6".
    var name: String { RuntimeCopy.name(of: summary) }
    /// "arm64 · verified", or the label of the state.
    var secondLine: String { RuntimeCopy.secondLine(of: summary) }
    /// How the state looks.
    var appearance: StatusAppearance { RuntimeAppearance.appearance(of: summary.state) }
    /// "Managed".
    var source: String { RuntimeCopy.source(summary.installMode) }
    /// `sha256:` and the short digest.
    var digest: String { RuntimeCopy.shortDigest(summary.artifactSha256) }
    /// The instances that use it.
    var usedBy: String { RuntimeCopy.usedBy(summary.usedBy) }
    /// Why Remove is locked.
    var removeReason: String { RuntimeCopy.removeReason(of: summary) }

    /// The rows of `runtimes`, each with the tile of the catalog provider that runs the
    /// same software, or a gray tile with the default mark of its name.
    /// - Parameters:
    ///   - runtimes: The runtimes, in table order.
    ///   - catalog: The providers of the catalog, or `nil` while it is not loaded.
    static func rows(of runtimes: [RuntimeSummary], catalog: [ProviderSummary]?)
        -> [RuntimeRowModel]
    {
        runtimes.map { RuntimeRowModel(summary: $0, tile: tile(for: $0, catalog: catalog)) }
    }

    /// The tile of the provider with ID `provider`, or else of the one that runs `software`.
    /// Without either the tile is gray with the default mark of `name`.
    static func tile(
        software: String, name: String, provider id: String? = nil, catalog: [ProviderSummary]?
    ) -> TileSpec {
        let found =
            catalog?.first(where: { $0.provider == id })
            ?? catalog?.first(where: { $0.software == software })
        guard let provider = found else {
            return TileSpec(mark: nil, tile: nil, name: name)
        }
        return TileSpec(mark: provider.mark, tile: provider.tile, name: name)
    }

    private static func tile(for runtime: RuntimeSummary, catalog: [ProviderSummary]?)
        -> TileSpec
    {
        tile(software: runtime.software, name: runtime.displayName, catalog: catalog)
    }
}
