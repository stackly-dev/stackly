import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The rows of the Catalog Trust step in every variant (spec 0016, AC-7; spec 0068). Each
/// row renders from a value `CatalogTrustContent.make` builds from a fixture catalog, so
/// no model or runtime is needed. The window level snapshot of the step joins in milestone 4.
extension SnapshotTests {
    struct CatalogTrustSnapshotTests {
        private static let utc = TimeZone(identifier: "UTC") ?? .gmt

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

        private static func row(
            info: CatalogInfo? = nil, error: ErrorBody? = nil, unsupported: Bool = false
        ) -> some View {
            CatalogTrustRow(
                content: CatalogTrustContent.make(
                    info: info, error: error, unsupported: unsupported,
                    runtimeInstallAvailable: false, locale: Locale(identifier: "en_US"),
                    calendar: Calendar(identifier: .gregorian), timeZone: utc))
        }

        private static func matrix() throws -> some View {
            let failed = ErrorBody(
                code: "catalog.unavailable", category: .unavailable,
                message: "the provider catalog is not available", retryable: false,
                resource: nil, fields: nil,
                hint: "run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR", details: nil,
                operationId: nil, rule: nil, items: nil)
            let official = try info()
            let outdated = try info(outdated: true)
            let development = try info(source: .development, trust: .development)
            let unknown = try info(source: .unrecognized, trust: .unrecognized)
            return VStack(spacing: Spacing.s14) {
                row(info: official)
                row(info: outdated)
                row(info: development)
                row(info: unknown)
                row(unsupported: true)
                row(error: failed)
                row()
                Text(OnboardingCopy.catalogNote)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            .frame(maxWidth: OnboardingFlow.columnWidth)
            .padding(Spacing.pageHorizontal)
            .background(StacklyColor.surfaceWindow)
        }

        @Test(
            .evidence(
                covers: ["REQ-024", "NFR-012"],
                expected: "Every row of the Catalog Trust table renders in every variant"
            ))
        func catalogTrustRows() async throws {
            try await assertSnapshot(
                Self.matrix(), named: "onboarding-catalog-trust-rows",
                size: .fitting(width: 600, narrowWidth: 420))
        }
    }
}
