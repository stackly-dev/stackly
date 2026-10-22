import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The key value rows of the detail panel: they read only the artifacts for this Mac
/// (spec 0017, *Layar Catalog (GUI)*).
struct ProviderFactsTests {
    let host = HostTarget(os: .macos, arch: .aarch64)
    let english = Locale(identifier: "en_US")

    @Test func theRowsOfAnAvailableProviderReadItsHostArtifact() throws {
        let facts = ProviderFacts(
            detail: try CatalogFixture.detail("nats"), host: host, locale: english)
        #expect(facts.licenses == ["Apache-2.0"])
        #expect(
            facts.licenseURL?.absoluteString
                == "https://github.com/nats-io/nats-server/blob/v2.15.1/LICENSE")
        #expect(facts.sources == ["github.com/nats-io"])
        #expect(facts.download == "6.8 MB")
        #expect(facts.disk == "Not listed")
        #expect(facts.requires == "macOS 13.0 or later on Apple Silicon")
        #expect(facts.dependencies == "None declared")
        #expect(facts.publisher == "Stackly")
        #expect(facts.revision == "1.0.0 · d115e2ad7cd0")
        #expect(facts.trust == "Official, bundled with Stackly")
    }

    @Test func anArtifactForAnotherArchitectureIsNotShown() throws {
        let facts = ProviderFacts(detail: try CatalogFixture.detail("intelonly"), host: host)
        #expect(facts.licenses.isEmpty)
        #expect(facts.download == "Not listed")
        #expect(facts.requires == "Not listed")
        #expect(facts.sources.isEmpty)
    }

    @Test func aBlockedArtifactStillNamesItsLicenseButHasNoSizeSourceOrRequirement() throws {
        let facts = ProviderFacts(detail: try CatalogFixture.detail("blockedhost"), host: host)
        #expect(facts.licenses == ["Apache-2.0"])
        #expect(facts.sources.isEmpty)
        #expect(facts.download == "Not listed")
        #expect(facts.requires == "Not listed")
    }

    @Test func twoSizesAreWrittenAsARange() throws {
        let detail = try CatalogFixture.detail("nats")
        let version = try #require(detail.versions?.first)
        let artifact = try #require(version.artifacts?.first)
        let bigger = CatalogArtifact(
            id: "x", os: artifact.os, arch: artifact.arch, status: .available, sha256: nil,
            minOs: "14.0", downloadSize: 48_100_000, installedSize: 120_000_000,
            license: "MIT", licenseUrl: nil, source: "example.com/x", blockedReason: nil,
            dependencies: [
                ArtifactDependency(
                    name: "openssl", kind: .bundled, version: "3.4.0", license: "Apache-2.0"),
                ArtifactDependency(name: "libc", kind: .system, version: nil, license: nil),
            ])
        let second = CatalogVersion(
            version: "2.16.0", line: "2.16", support: version.support, install: version.install,
            artifacts: [bigger])
        let two = ProviderDetail(
            summary: detail.summary, software: detail.software,
            manifestSha256: detail.manifestSha256, capabilities: detail.capabilities,
            versions: [version, second], platformNotes: detail.platformNotes, note: nil,
            issues: nil)
        let facts = ProviderFacts(detail: two, host: host, locale: english)
        #expect(facts.download == "6.8 MB to 48.1 MB")
        #expect(facts.disk == "120 MB")
        #expect(facts.requires == "macOS 13.0 or later on Apple Silicon")
        #expect(facts.licenses == ["Apache-2.0", "MIT"])
        #expect(facts.licenseURL == nil)
        #expect(facts.sources == ["github.com/nats-io", "example.com/x"])
        #expect(facts.dependencies == "openssl 3.4.0 (Apache-2.0)")
    }

    @Test func aDeclaredEmptyDependencyListReadsNone() throws {
        let detail = try CatalogFixture.detail("nats")
        let version = try #require(detail.versions?.first)
        let artifact = try #require(version.artifacts?.first)
        let plain = CatalogArtifact(
            id: artifact.id, os: artifact.os, arch: artifact.arch, status: .available,
            sha256: nil, minOs: nil, downloadSize: nil, installedSize: nil, license: nil,
            licenseUrl: nil, source: nil, blockedReason: nil, dependencies: [])
        let changed = ProviderDetail(
            summary: detail.summary, software: nil, manifestSha256: nil, capabilities: nil,
            versions: [
                CatalogVersion(
                    version: version.version, line: version.line, support: version.support,
                    install: version.install, artifacts: [plain])
            ], platformNotes: nil, note: nil, issues: nil)
        let facts = ProviderFacts(detail: changed, host: host)
        #expect(facts.dependencies == "None")
        #expect(facts.revision == "1.0.0")
    }

    @Test func aLicenseLinkMustBeAWebAddress() throws {
        let detail = try CatalogFixture.detail("nats")
        let version = try #require(detail.versions?.first)
        let artifact = try #require(version.artifacts?.first)
        let local = CatalogArtifact(
            id: artifact.id, os: artifact.os, arch: artifact.arch, status: .available,
            sha256: nil, minOs: nil, downloadSize: nil, installedSize: nil, license: "MIT",
            licenseUrl: "file:///etc/passwd", source: nil, blockedReason: nil,
            dependencies: nil)
        let changed = ProviderDetail(
            summary: detail.summary, software: nil, manifestSha256: nil, capabilities: nil,
            versions: [
                CatalogVersion(
                    version: version.version, line: version.line, support: version.support,
                    install: version.install, artifacts: [local])
            ], platformNotes: nil, note: nil, issues: nil)
        #expect(ProviderFacts(detail: changed, host: host).licenseURL == nil)
    }

    @Test func foreignValuesAreWrittenAsNeutralText() throws {
        let facts = ProviderFacts(detail: try CatalogFixture.unknownDetail(), host: host)
        #expect(facts.trust == "Unknown trust")
        #expect(facts.licenses.isEmpty)
    }
}
