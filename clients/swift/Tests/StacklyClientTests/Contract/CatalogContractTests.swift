import Foundation
import StacklyClient
import Testing

/// The `catalog` area of protocol 1.6 from the shared fixtures (spec 0017
/// AC-13): both replies and the catalog fixtures decode, foreign values of
/// the open enums read as `.unrecognized`, and the catalog errors carry
/// their details and static hints.
struct CatalogContractTests {
    static func reply(_ file: String) throws -> Reply {
        guard case .response(let response) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return response.reply
    }

    static func catalog<Value: Decodable>(_ type: Value.Type, _ file: String) throws -> Value {
        try StacklyJSON.decoder().decode(type, from: Fixture.data("catalog/\(file)"))
    }

    static func error(_ file: String) throws -> ErrorBody {
        guard case .error(let frame) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return frame.error
    }

    @Test func theListReplyCarriesTheCatalogFactsAndEveryEntryInDisplayOrder() throws {
        guard
            case .catalogListProviders(let list) = try Self.reply(
                "frame-response-catalog-list-providers.json")
        else {
            Issue.record("expected a catalog.list_providers reply")
            return
        }
        #expect(list == (try Self.catalog(ListProvidersReply.self, "list-providers.json")))
        #expect(list.catalog.source == .bundled)
        #expect(list.catalog.trust == .official)
        #expect(list.catalog.host == HostTarget(os: .macos, arch: .aarch64))
        #expect(list.catalog.outdated == false)
        #expect(list.catalog.invalidCount == 2)
        let items = try #require(list.items)
        #expect(
            items.map(\.state) == [
                .blocked, .unsupported, .available, .invalid, .invalid,
                .planned, .planned, .planned, .planned, .planned,
            ])
        let nats = try #require(items.first { $0.provider == "official/nats" })
        #expect(nats.software == "nats")
        #expect(nats.versions == ["2.15.1"])
        #expect(nats.versionCount == 1)
        #expect(nats.tile == .green)
        let planned = try #require(items.first { $0.provider == "official/postgresql" })
        #expect(planned.software == nil)
        #expect(planned.stateReason == .providerPlanned)
    }

    @Test func theDetailReplyCarriesCapabilitiesTheMatrixAndTheHostArtifactDigest() throws {
        guard
            case .catalogGetProvider(let detail) = try Self.reply(
                "frame-response-catalog-get-provider.json")
        else {
            Issue.record("expected a catalog.get_provider reply")
            return
        }
        #expect(detail == (try Self.catalog(ProviderDetail.self, "provider-nats.json")))
        #expect(
            detail.capabilities?.map(\.capability) == [
                .multiInstance, .persistence, .reload, .metrics, .tls, .coldBackup,
                .onlineBackup, .logicalBackup,
            ])
        #expect(detail.capabilities?.first { $0.capability == .reload }?.supported == false)
        let version = try #require(detail.versions?.first)
        #expect(version.support?.map(\.os) == [.macos, .linux, .windows])
        #expect(version.support?.map(\.status) == [.unverified, .unsupported, .planned])
        #expect(version.install == InstallVerdict(installable: true, reason: nil))
        let artifact = try #require(version.artifacts?.first)
        #expect(artifact.sha256?.count == 64)
        #expect(artifact.downloadSize == 6_785_953)
        #expect(artifact.source == "github.com/nats-io")
        #expect(artifact.dependencies == nil)
    }

    @Test func refusedEntriesCarryTheirReasonOnTheCardAndInTheMatrix() throws {
        let intel = try Self.catalog(ProviderDetail.self, "provider-intelonly.json")
        #expect(intel.summary.stateReason == .noBuildForArch)
        #expect(intel.versions?.first?.support?.first?.arches == [.x8664])
        #expect(intel.versions?.first?.install.reason == .noBuildForArch)

        let blocked = try Self.catalog(ProviderDetail.self, "provider-blockedhost.json")
        #expect(blocked.summary.state == .blocked)
        #expect(blocked.summary.stateNote == "the upstream build fails the closure audit")
        let artifact = try #require(blocked.versions?.first?.artifacts?.first)
        #expect(artifact.status == .blocked)
        #expect(artifact.sha256 == nil)
        #expect(artifact.blockedReason == blocked.summary.stateNote)
    }

    @Test func invalidAndPlannedEntriesCarryIssuesOrANoteButNoVersions() throws {
        let zenith = try Self.catalog(ProviderDetail.self, "provider-zenith.json")
        #expect(zenith.summary.category == nil)
        #expect(zenith.issues?.first?.code == "catalog_limit")
        #expect(zenith.issues?.first?.reason == "total_bytes")
        let broken = try Self.catalog(ProviderDetail.self, "provider-broken.json")
        #expect(broken.issues?.first?.code == "catalog_digest_mismatch")
        #expect(broken.issues?.first?.reason == nil)
        #expect(broken.capabilities == [])

        let planned = try Self.catalog(ProviderDetail.self, "provider-postgresql.json")
        #expect(planned.note == "Planned for a later release.")
        #expect(planned.versions == [])
        #expect(planned.manifestSha256 == nil)
    }

    @Test func foreignCatalogValuesDecodeAsUnrecognized() throws {
        guard
            case .catalogGetProvider(let detail) = try Self.reply(
                "frame-response-catalog-get-provider-unknown-values.json")
        else {
            Issue.record("expected a catalog.get_provider reply")
            return
        }
        #expect(detail.summary.category == .unrecognized)
        #expect(detail.summary.state == .unrecognized)
        #expect(detail.summary.stateReason == .unrecognized)
        #expect(detail.summary.trust == .unrecognized)
        #expect(detail.summary.tile == .unrecognized)
        #expect(detail.capabilities?.last?.capability == .unrecognized)
        let version = try #require(detail.versions?.first)
        #expect(version.support?.first?.arches == [.unrecognized])
        #expect(version.support?.first?.status == .unrecognized)
        #expect(version.support?[1].os == .unrecognized)
        #expect(version.install.reason == .unrecognized)
        let artifact = try #require(version.artifacts?.first)
        #expect(artifact.os == .unrecognized)
        #expect(artifact.arch == .unrecognized)
        #expect(artifact.status == .unrecognized)
        #expect(artifact.dependencies?.first?.kind == .unrecognized)
        #expect(detail.platformNotes?.first?.os == .unrecognized)

        guard
            case .catalogListProviders(let list) = try Self.reply(
                "frame-response-catalog-list-providers-unknown-values.json")
        else {
            Issue.record("expected a catalog.list_providers reply")
            return
        }
        #expect(list.catalog.source == .unrecognized)
        #expect(list.catalog.trust == .unrecognized)
        #expect(list.catalog.host == HostTarget(os: .unrecognized, arch: .unrecognized))
    }

    @Test func catalogErrorsCarryTheirDetailsAndAStaticHint() throws {
        let unavailable = try Self.error("frame-error-catalog-unavailable.json")
        #expect(unavailable.category == .unavailable)
        #expect(unavailable.details?.first == ErrorDetail(key: "reason", value: "not_configured"))
        #expect(unavailable.hint == "run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR")

        let notFound = try Self.error("frame-error-catalog-provider-not-found.json")
        #expect(notFound.category == .notFound)
        #expect(notFound.details?.last == ErrorDetail(key: "suggestion", value: "official/nats"))
        #expect(notFound.hint == "use the full provider ID, including its namespace")

        let refused = try Self.error("frame-error-catalog-not-installable.json")
        #expect(refused.category == .unsupported)
        #expect(
            refused.details?.map(\.key) == [
                "provider", "version", "target", "availableTargets", "reason",
            ])
        #expect(refused.hint?.contains("{") == false)
    }

    @Test func getProviderParamsEncodeTheProviderID() throws {
        let request = try Fixture.ipc(Request.self, "request-catalog-get-provider.json")
        #expect(request.call == .catalogGetProvider(GetProviderParams(provider: "official/nats")))
        let list = try Fixture.ipc(Request.self, "request-catalog-list-providers.json")
        #expect(list.call == .catalogListProviders)
    }
}
