import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The words of the Catalog screen: spec 0017 *Copy* and *Nilai asing* (AC-8, AC-9, AC-12).
struct CatalogCopyTests {
    @Test func everyReasonHasTheTextOfTheCopyTable() {
        #expect(CatalogCopy.reasonText(.untested) == "Not tested on this Mac yet.")
        #expect(CatalogCopy.reasonText(.noBuild, os: .linux) == "No Linux build in this catalog.")
        #expect(
            CatalogCopy.reasonText(.noBuildForArch, os: .macos, arches: [.x8664])
                == "No build for Apple Silicon. Builds exist for Intel.")
        #expect(
            CatalogCopy.reasonText(.noBuildForHost)
                == "No build for macOS on Apple Silicon. Stackly can’t install it on this Mac.")
        #expect(
            CatalogCopy.reasonText(.platformNote, os: .windows, note: "Windows is planned.")
                == "Windows is planned.")
        #expect(
            CatalogCopy.reasonText(.artifactBlocked, note: "fails the closure audit")
                == "Blocked: fails the closure audit")
        #expect(CatalogCopy.reasonText(.fixtureOnly) == "Test fixture, not installable.")
        #expect(CatalogCopy.reasonText(.providerPlanned) == "Not in this release.")
        #expect(
            CatalogCopy.reasonText(.packageInvalid) == "The provider package failed validation.")
        #expect(
            CatalogCopy.reasonText(.unrecognized)
                == "Reason not recognized by this version of Stackly.")
    }

    @Test func architecturesAreNamedByTheOSTheyRunOn() {
        #expect(CatalogCopy.archName(.aarch64, on: .macos) == "Apple Silicon")
        #expect(CatalogCopy.archName(.x8664, on: .macos) == "Intel")
        #expect(CatalogCopy.archName(.aarch64, on: .linux) == "ARM64")
        #expect(CatalogCopy.archName(.x8664, on: .windows) == "x86 64")
        #expect(CatalogCopy.archName(.unrecognized, on: .macos) == "Unknown")
        #expect(CatalogCopy.osName(.unrecognized) == "Unknown")
    }

    @Test func aForeignValueIsWrittenAsNeutralTextNeverGuessed() {
        #expect(CatalogCopy.categoryName(.unrecognized) == "Other")
        #expect(CatalogCopy.categoryName(nil) == "Other")
        #expect(CatalogCopy.trustText(.unrecognized) == "Unknown trust")
        #expect(CatalogCopy.trustTitle(.unrecognized) == "Unknown trust")
        #expect(CatalogCopy.sourceName(.unrecognized) == "Unknown source")
        #expect(CatalogCopy.capabilityLabel(.unrecognized) == "Unknown capability")
    }

    @Test func capabilitiesUseTheLabelsOfTheCopyTable() {
        let labels = [
            ProviderCapability.multiInstance, .persistence, .reload, .metrics, .tls, .coldBackup,
            .onlineBackup, .logicalBackup,
        ].map(CatalogCopy.capabilityLabel)
        #expect(
            labels == [
                "Multiple instances", "Persistent data", "Reload without restart", "Metrics",
                "TLS", "Cold backup", "Online backup", "Logical backup",
            ])
    }

    @Test func sizesAreDecimalWithTheCLIsUnits() {
        let english = Locale(identifier: "en_US")
        #expect(CatalogCopy.sizeText(6_785_953, locale: english) == "6.8 MB")
        #expect(CatalogCopy.sizeText(48_100_000, locale: english) == "48.1 MB")
        #expect(CatalogCopy.sizeText(1_200_000_000, locale: english) == "1.2 GB")
        #expect(CatalogCopy.sizeText(1_000_000, locale: english) == "1 MB")
    }

    @Test func theSubtitleSaysWhenTheCatalogWasPublishedAndNeverThatItIsNewest() throws {
        let info = try CatalogFixture.list().catalog
        let locale = Locale(identifier: "en_US")
        let utc = try #require(TimeZone(identifier: "UTC"))
        let subtitle = CatalogCopy.subtitle(
            info: info, locale: locale, calendar: Calendar(identifier: .gregorian), timeZone: utc)
        #expect(subtitle == "Official · updated Oct 20, 2026")
        for word in ["latest", "up to date", "newer"] {
            #expect(subtitle.localizedCaseInsensitiveContains(word) == false)
        }
    }

    @Test func aDevelopmentCatalogIsNamedAsOne() throws {
        let fixture = try CatalogFixture.list().catalog
        let info = CatalogInfo(
            revision: fixture.revision, publishedAt: fixture.publishedAt,
            loadedAt: fixture.loadedAt, contentSha256: fixture.contentSha256,
            source: .development, trust: .development, host: fixture.host, outdated: false,
            invalidCount: 0)
        let utc = try #require(TimeZone(identifier: "UTC"))
        #expect(
            CatalogCopy.subtitle(
                info: info, locale: Locale(identifier: "en_US"),
                calendar: Calendar(identifier: .gregorian), timeZone: utc)
                == "Development catalog · updated Oct 20, 2026")
        #expect(CatalogCopy.sourceName(.development) == "the development catalog")
        #expect(CatalogCopy.sourceName(.bundled) == "the bundled catalog")
    }

    @Test func aCardsMetaLineSaysWhatTheEntryIs() throws {
        let items = try #require(CatalogFixture.list().items)
        let byID = Dictionary(uniqueKeysWithValues: items.map { ($0.provider, $0) })
        let nats = try #require(byID["official/nats"])
        let mysql = try #require(byID["official/mysql"])
        let broken = try #require(byID["official/broken"])
        #expect(nats.metaLineText == "Messaging · Apache-2.0")
        #expect(mysql.metaLineText == "Not in this release")
        #expect(broken.metaLineText == "Failed validation")
    }

}

extension ProviderSummary {
    fileprivate var metaLineText: String { CatalogCopy.metaLine(of: self) }
}
