#if DEBUG
    import StacklyClient
    import SwiftUI

    /// A daemon for the Catalog previews: it answers the two catalog methods from a few
    /// providers built in code, the same shapes the snapshots read from the contract
    /// fixtures.
    enum CatalogPreviewSamples {
        /// The providers of the previews: an available, an unsupported, an invalid, and
        /// a planned one.
        static var providers: [ProviderSummary] {
            [
                summary(
                    "official/nats", "NATS Server", .messaging, .available, "Na", .green,
                    description: "NATS messaging server with JetStream, one server per instance.",
                    versions: ["2.15.1"], licenses: ["Apache-2.0"]),
                summary(
                    "official/intelonly", "NATS Intel Build", .messaging, .unsupported, "Na",
                    .gray, reason: .noBuildForArch,
                    description: "NATS messaging server with JetStream, one server per instance."),
                summary(
                    "official/broken", "official/broken", .cache, .invalid, "Br", .gray,
                    reason: .packageInvalid),
                summary(
                    "official/mysql", "MySQL", .database, .planned, "My", .teal,
                    reason: .providerPlanned,
                    description: "Relational server with isolated datadir, socket and tmpdir."),
            ]
        }

        private static func summary(
            _ id: String, _ name: String, _ category: ProviderCategory,
            _ state: CatalogEntryState, _ mark: String, _ tile: TileColor,
            reason: SupportReason? = nil, description: String? = nil,
            versions: [String] = [], licenses: [String] = []
        ) -> ProviderSummary {
            ProviderSummary(
                provider: id, revision: 1, displayName: name,
                software: id.split(separator: "/").last.map(String.init), category: category,
                description: description, state: state, stateReason: reason, stateNote: nil,
                providerRevision: state == .planned || state == .invalid ? nil : "1.0.0",
                publisher: "Stackly", trust: .official, versions: versions,
                versionCount: UInt64(versions.count), licenses: licenses, mark: mark, tile: tile)
        }

        /// A detail for `summary`: one version with the three OS cells of its state.
        static func detail(for summary: ProviderSummary) -> ProviderDetail {
            let ready = summary.state == .available
            let macos = TargetSupport(
                os: .macos, arches: ready ? [.aarch64] : [],
                status: ready ? .unverified : .unsupported,
                reason: ready ? .untested : summary.stateReason, note: nil)
            let others = [TargetOs.linux, .windows].map {
                TargetSupport(
                    os: $0, arches: [], status: .unsupported, reason: .noBuild, note: nil)
            }
            let artifact = CatalogArtifact(
                id: "\(summary.provider)-macos-aarch64", os: .macos, arch: .aarch64,
                status: .available, sha256: nil, minOs: "13.0", downloadSize: 6_785_953,
                installedSize: nil, license: "Apache-2.0", licenseUrl: nil,
                source: "github.com/nats-io", blockedReason: nil, dependencies: nil)
            let capabilities = [
                ProviderCapability.multiInstance, .persistence, .reload, .metrics, .tls,
                .coldBackup,
                .onlineBackup, .logicalBackup,
            ].map {
                CapabilityStatus(capability: $0, supported: $0 == .persistence || $0 == .metrics)
            }
            switch summary.state {
            case .available, .blocked, .unsupported, .unrecognized:
                return ProviderDetail(
                    summary: summary, software: summary.software,
                    manifestSha256: String(repeating: "ab", count: 32),
                    capabilities: capabilities,
                    versions: [
                        CatalogVersion(
                            version: "2.15.1", line: "2.15", support: [macos] + others,
                            install: InstallVerdict(
                                installable: ready, reason: summary.stateReason),
                            artifacts: ready ? [artifact] : [])
                    ],
                    platformNotes: [], note: nil, issues: [])
            case .planned:
                return ProviderDetail(
                    summary: summary, software: nil, manifestSha256: nil, capabilities: [],
                    versions: [], platformNotes: [], note: nil, issues: [])
            case .invalid:
                return ProviderDetail(
                    summary: summary, software: nil, manifestSha256: nil, capabilities: [],
                    versions: [], platformNotes: [], note: nil,
                    issues: [
                        CatalogIssue(
                            file: "catalog.json", location: "providers[1].manifestSha256",
                            code: "catalog_digest_mismatch",
                            message: "manifestSha256 does not match the package files", reason: nil)
                    ])
            }
        }

        /// A catalog store over the sample providers, loaded.
        @MainActor
        static func store(providers: [ProviderSummary] = providers) async -> CatalogStore {
            let reply = ListProvidersReply(catalog: .fixture, items: providers)
            let store = CatalogStore(
                call: { call throws(ClientError) in
                    switch call {
                    case .catalogListProviders:
                        return .catalogListProviders(reply)
                    case .catalogGetProvider(let params):
                        guard let found = providers.first(where: { $0.provider == params.provider })
                        else { throw .notSent(ClientErrorCatalog.notConnected()) }
                        return .catalogGetProvider(detail(for: found))
                    default:
                        throw .notSent(ClientErrorCatalog.notConnected())
                    }
                })
            await store.store.resnapshot()
            return store
        }
    }

    private struct CatalogPreview: View {
        let appearance: FixtureAppearance
        var selection = "official/nats"
        var filter = CatalogFilter()
        @State private var store: CatalogStore?

        var body: some View {
            Group {
                if let store {
                    CatalogPage(
                        catalog: store, waiting: nil, hasConnection: true, reconnect: {},
                        openDiagnostics: {}, selection: selection, filter: filter
                    )
                    .actionHost()
                }
            }
            .task { store = await CatalogPreviewSamples.store() }
            .fixtureAppearance(appearance)
        }
    }

    #Preview("Light") {
        CatalogPreview(appearance: .light).frame(width: 1044, height: 748)
    }

    #Preview("Dark") {
        CatalogPreview(appearance: .dark).frame(width: 1044, height: 748)
    }

    #Preview("Narrow width") {
        CatalogPreview(appearance: .light, selection: "official/intelonly")
            .frame(width: AppShell.contentMinWidth, height: 748)
    }

    #Preview("No results") {
        CatalogPreview(appearance: .light, filter: CatalogFilter(query: "zzz"))
            .frame(width: 1044, height: 748)
    }
#endif
