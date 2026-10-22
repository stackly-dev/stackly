import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The Catalog Trust row: one case per row of the table, the store behind it, and the copy
/// (spec 0016, AC-7 and AC-9).
@Suite(.timeLimit(.minutes(1)))
@MainActor
struct CatalogTrustContentTests {
    private static let utc = TimeZone(identifier: "UTC") ?? .gmt
    private static let locale = Locale(identifier: "en_US")
    private static let calendar = Calendar(identifier: .gregorian)

    private static func info(
        source: CatalogSourceKind = .bundled, trust: TrustLevel = .official,
        outdated: Bool = false
    ) throws -> CatalogInfo {
        let base = try #require(CatalogFixture.list().catalog)
        return CatalogInfo(
            revision: base.revision, publishedAt: base.publishedAt, loadedAt: base.loadedAt,
            contentSha256: base.contentSha256, source: source, trust: trust, host: base.host,
            outdated: outdated, invalidCount: base.invalidCount)
    }

    private static func error(hint: String? = nil) -> ErrorBody {
        ErrorBody(
            code: "catalog.unavailable", category: .unavailable,
            message: "the provider catalog is not available", retryable: false, resource: nil,
            fields: nil, hint: hint, details: nil, operationId: nil, rule: nil, items: nil)
    }

    private static func make(
        info: CatalogInfo? = nil, error: ErrorBody? = nil, unsupported: Bool = false,
        runtimeInstallAvailable: Bool = false
    ) -> CatalogTrustContent {
        CatalogTrustContent.make(
            info: info, error: error, unsupported: unsupported,
            runtimeInstallAvailable: runtimeInstallAvailable, locale: locale,
            calendar: calendar, timeZone: utc)
    }

    // MARK: - The table

    @Test func theOfficialCatalogIsLabeledOfficialWithItsRevisionAndDate() throws {
        let info = try Self.info()
        let row = Self.make(info: info)
        #expect(row.label == "official")
        #expect(row.detail == "Maintained by the Stackly project and bundled with this app.")
        #expect(row.revision == "Revision \(info.revision) · published Oct 20, 2026")
        #expect(row.toggle == "The official catalog is always on.")
    }

    @Test func theDigestSentenceFollowsTheBuildConstant() throws {
        let off = Self.make(info: try Self.info(), runtimeInstallAvailable: false)
        let on = Self.make(info: try Self.info(), runtimeInstallAvailable: true)
        #expect(off.detail.contains("pinned digests") == false)
        #expect(
            on.detail
                == "Maintained by the Stackly project and bundled with this app. Runtimes are checked against pinned digests."
        )
    }

    @Test func anOutdatedCatalogAddsTheAgeToTheRevisionLine() throws {
        let info = try Self.info(outdated: true)
        let row = Self.make(info: info)
        #expect(
            row.revision
                == "Revision \(info.revision) · published Oct 20, 2026 · older than 180 days"
        )
        #expect(row.label == "official")
    }

    @Test func aDevelopmentCatalogIsNotCalledOfficial() throws {
        let row = Self.make(info: try Self.info(source: .development, trust: .development))
        #expect(row.label == "development")
        #expect(
            row.detail
                == "A development catalog from STACKLY_CATALOG_DIR. It isn’t the official catalog.")
        #expect(row.toggle == "The development catalog is always on.")
        #expect(row.revision?.hasPrefix("Revision ") == true)
    }

    @Test func anUnrecognizedSourceIsUnknown() throws {
        let row = Self.make(info: try Self.info(source: .unrecognized, trust: .development))
        #expect(row.label == "Unknown source")
        #expect(row.detail == "This version of Stackly doesn’t recognize this catalog source.")
        #expect(row.toggle == "This catalog is always on.")
    }

    @Test func anUnrecognizedTrustSaysSoAndIsNeverOfficial() throws {
        let row = Self.make(info: try Self.info(source: .bundled, trust: .unrecognized))
        #expect(row.label == "Unknown source")
        #expect(
            row.detail
                == "This version of Stackly doesn’t recognize this catalog source. Unknown trust.")
    }

    @Test func anUnsupportedDaemonHasNoToggleAndSaysToUpdate() {
        let row = Self.make(unsupported: true)
        #expect(row.label == "Catalog")
        #expect(
            row.detail == "This version of stacklyd doesn’t provide the catalog. Update Stackly.")
        #expect(row.revision == nil)
        #expect(row.toggle == nil)
    }

    @Test func anErrorShowsItsMessageAndHint() {
        let row = Self.make(error: Self.error(hint: "Start the daemon, then try again."))
        #expect(row.label == "Catalog")
        #expect(row.detail == "The catalog isn’t available: the provider catalog is not available.")
        #expect(row.revision == "Start the daemon, then try again.")
        #expect(row.toggle == nil)
    }

    @Test func anErrorWithoutAHintHasNoSecondLine() {
        #expect(Self.make(error: Self.error()).revision == nil)
    }

    @Test func unsupportedBeatsAnErrorWhenThereIsNoCatalog() {
        let row = Self.make(error: Self.error(), unsupported: true)
        #expect(row.detail.contains("Update Stackly"))
    }

    @Test func beforeTheDaemonAnswersTheRowWaitsWithoutAToggle() {
        let row = Self.make()
        #expect(row.label == "Catalog")
        #expect(
            row.detail == "Stackly shows where its catalog comes from once stacklyd is running.")
        #expect(row.toggle == nil)
    }

    @Test func aCatalogAlreadyReportedWinsOverALaterError() throws {
        let row = Self.make(info: try Self.info(), error: Self.error(), unsupported: true)
        #expect(row.label == "official")
        #expect(row.toggle != nil)
    }

    // MARK: - No community, no promises

    @Test func noRowMentionsACommunityCatalogOrNewness() throws {
        let rows = [
            Self.make(info: try Self.info(), runtimeInstallAvailable: true),
            Self.make(info: try Self.info(outdated: true)),
            Self.make(info: try Self.info(source: .development, trust: .development)),
            Self.make(info: try Self.info(source: .unrecognized, trust: .unrecognized)),
            Self.make(unsupported: true), Self.make(error: Self.error(hint: "Try again.")),
            Self.make(),
        ]
        for row in rows {
            let text = [row.label, row.detail, row.revision ?? "", row.toggle ?? ""]
                .joined(separator: " ")
            for phrase in ["community", "Signed manifests", "up to date", "latest", "newest"] {
                #expect(text.localizedCaseInsensitiveContains(phrase) == false, "“\(phrase)”")
            }
        }
        #expect(OnboardingCopy.catalogNote.localizedCaseInsensitiveContains("community") == false)
    }

    @Test func theTrustNoteIsTheTable() {
        #expect(
            OnboardingCopy.catalogNote
                == "Trust tells you where a provider comes from. It isn’t a sandbox: every server runs with your user’s permissions."
        )
    }

    // MARK: - The store behind the row

    private func content(_ store: CatalogStore) -> CatalogTrustContent {
        CatalogTrustContent.make(
            info: store.info, error: store.store.error, unsupported: store.store.unsupported,
            runtimeInstallAvailable: false, locale: Self.locale, calendar: Self.calendar,
            timeZone: Self.utc)
    }

    @Test func aStoreBeforeItsFirstSnapshotShowsTheWaitingRow() throws {
        let store = try CatalogScript.fixture().store()
        #expect(content(store).detail == OnboardingCopy.catalogDetailWaiting)
    }

    @Test func theFixtureCatalogShowsTheOfficialRow() async throws {
        let script = try CatalogScript.fixture()
        let store = script.store()
        await store.store.resnapshot()
        let row = content(store)
        let revision = try #require(script.list?.catalog).revision
        #expect(row.label == "official")
        #expect(row.revision == "Revision \(revision) · published Oct 20, 2026")
        #expect(row.toggle != nil)
    }

    @Test func aFailingFirstSnapshotShowsTheErrorRow() async throws {
        let script = try CatalogScript.fixture()
        script.failure = .rejected(Self.error(hint: "Start the daemon."))
        let store = script.store()
        await store.store.resnapshot()
        let row = content(store)
        #expect(row.label == "Catalog")
        #expect(row.detail.hasPrefix("The catalog isn’t available: "))
        #expect(row.revision == "Start the daemon.")
        #expect(row.toggle == nil)
    }

    @Test func aDaemonWithoutTheMethodShowsTheUnsupportedRow() async throws {
        let script = try CatalogScript.fixture()
        script.failure = .rejected(
            ErrorBody(
                code: "request.unknown_method", category: .unsupported, message: "unknown method",
                retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
                operationId: nil, rule: nil, items: nil))
        let store = script.store()
        await store.store.resnapshot()
        #expect(content(store).detail == OnboardingCopy.catalogDetailUnsupported)
    }

    @Test func aRefreshThatFailsKeepsTheOfficialRow() async throws {
        let script = try CatalogScript.fixture()
        let store = script.store()
        await store.store.resnapshot()
        script.failure = .rejected(Self.error())
        await store.store.resnapshot()
        #expect(store.store.error != nil)
        let row = content(store)
        #expect(row.label == "official")
        #expect(row.toggle != nil)
    }
}
