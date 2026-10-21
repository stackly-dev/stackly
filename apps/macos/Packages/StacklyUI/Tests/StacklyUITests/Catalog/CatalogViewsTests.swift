import StacklyClient
import Testing

@testable import StacklyUI

/// What the matrix, the capability list, the cards, and the panel say (spec 0017, AC-10
/// and AC-11).
struct CatalogViewsTests {
    // MARK: - Matrix

    @Test func theNotesUnderTheMatrixListEachOSStatusAndReasonOnce() throws {
        let nats = try CatalogFixture.detail("nats")
        let notes = VersionMatrix.notes(of: try #require(nats.versions))
        #expect(notes.count == 3)
        #expect(notes[0] == "macOS: Not tested on this Mac yet.")
        #expect(notes[1] == "Linux: No Linux build in this catalog.")
        #expect(notes[2].hasPrefix("Windows: "))
    }

    @Test func twoVersionsWithTheSameReasonAddOneNote() throws {
        let nats = try CatalogFixture.detail("nats")
        let version = try #require(nats.versions?.first)
        let other = CatalogVersion(
            version: "2.16.0", line: "2.16", support: version.support, install: version.install,
            artifacts: version.artifacts)
        #expect(VersionMatrix.notes(of: [version, other]) == VersionMatrix.notes(of: [version]))
    }

    @Test func aBlockedCellExplainsWhyAndAnIntelOnlyCellNamesTheBuilds() throws {
        let blocked = try CatalogFixture.detail("blockedhost")
        #expect(
            VersionMatrix.notes(of: try #require(blocked.versions)).first?.hasPrefix(
                "macOS: Blocked: ") == true)
        let intel = try CatalogFixture.detail("intelonly")
        #expect(
            VersionMatrix.notes(of: try #require(intel.versions)).first
                == "macOS: No build for Apple Silicon. Builds exist for Intel.")
    }

    @Test func aCellWithoutAReasonAddsNoNote() {
        let cell = TargetSupport(
            os: .macos, arches: [.aarch64], status: .supported, reason: nil, note: nil)
        let version = CatalogVersion(
            version: "1.0", line: "1", support: [cell],
            install: InstallVerdict(installable: true, reason: nil), artifacts: nil)
        #expect(VersionMatrix.notes(of: [version]).isEmpty)
        #expect(CatalogCopy.spokenText(for: cell) == "macOS, Tested")
    }

    // MARK: - Capabilities

    @Test func theCapabilityListStartsWithTheBaselineThenTheCapabilitiesInOrder() throws {
        let nats = try CatalogFixture.detail("nats")
        let rows = CapabilityList.rows(for: try #require(nats.capabilities))
        #expect(rows.count == 11)
        #expect(Array(rows.prefix(3).map(\.text)) == ["Lifecycle", "Readiness probe", "Logs"])
        #expect(rows.prefix(3).map(\.supported) == [true, true, true])
        #expect(rows[3].text == "Multiple instances")
        #expect(rows[5].text == "Reload without restart · not supported")
        #expect(rows[5].spokenText == "Reload without restart, not supported")
        #expect(rows[6].spokenText == "Metrics, supported")
        #expect(rows.last?.text == "Logical backup · not supported")
    }

    @Test func aForeignCapabilityKeepsItsValueUnderANeutralLabel() throws {
        let rows = CapabilityList.rows(
            for: try #require(CatalogFixture.unknownDetail().capabilities))
        #expect(rows.last?.text == "Unknown capability")
        #expect(rows.last?.supported == true)
    }

    @Test func aPlannedProviderHasNoCapabilityToList() throws {
        let postgresql = try CatalogFixture.detail("postgresql")
        #expect(postgresql.capabilities?.isEmpty ?? true)
        #expect(postgresql.versions?.isEmpty ?? true)
        #expect(postgresql.note == "Planned for a later release.")
    }

    // MARK: - Cards

    @Test func aCardIsSpokenWithItsNameCategoryAndStatus() throws {
        let items = try #require(CatalogFixture.list().items)
        let nats = try #require(items.first { $0.provider == "official/nats" })
        #expect(ProviderCard.spokenLabel(for: nats) == "NATS Server, Messaging, Available")
        let mysql = try #require(items.first { $0.provider == "official/mysql" })
        #expect(ProviderCard.spokenLabel(for: mysql) == "MySQL, Database, Planned")
        let blocked = try #require(items.first { $0.provider == "official/blockedhost" })
        #expect(ProviderCard.spokenLabel(for: blocked) == "NATS Blocked Build, Messaging, Blocked")
        let zenith = try #require(items.first { $0.provider == "official/zenith" })
        #expect(ProviderCard.spokenLabel(for: zenith) == "official/zenith, Other, Invalid")
    }

    @Test func aForeignStateIsSpokenAsUnrecognized() throws {
        let nats = try #require(CatalogFixture.unknownList().items?.first)
        #expect(nats.state == .unrecognized)
        #expect(ProviderCard.spokenLabel(for: nats) == "NATS Server, Other, Unrecognized State")
        #expect(nats.tile == .unrecognized)
    }

    // MARK: - Panel

    @Test func anInvalidProviderListsAtMostFiveIssuesWithUniqueNames() {
        let issues = (0..<8).map { _ in
            CatalogIssue(
                file: "provider.yaml", location: "metadata.id", code: "catalog_id_mismatch",
                message: "metadata.id differs from the catalog entry", reason: nil)
        }
        let items = ProviderDetailPanel.issueItems(issues)
        #expect(items.count == 5)
        #expect(Set(items.map(\.name)).count == 5)
        #expect(items.allSatisfy { $0.reason == "metadata.id differs from the catalog entry" })
    }

    @Test func theBrokenFixturesIssueSurvivesIntoTheBanner() throws {
        let broken = try CatalogFixture.detail("broken")
        let items = ProviderDetailPanel.issueItems(try #require(broken.issues))
        #expect(items.count == 1)
    }
}
