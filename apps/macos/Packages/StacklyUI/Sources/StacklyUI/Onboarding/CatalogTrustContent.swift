import Foundation
import StacklyClient

/// What the Catalog Trust row shows (spec 0016, *Baris Catalog Trust*). Pure, so each row of
/// the table is tested without a view.
nonisolated struct CatalogTrustContent: Equatable {
    /// The row title.
    let label: String
    /// The sentence under the title.
    let detail: String
    /// The revision line, the hint of an error, or `nil`.
    let revision: String?
    /// The label and help of the always on toggle; `nil` means the row has no toggle.
    let toggle: String?

    /// Builds the row. The first matching case wins, so a catalog the daemon already
    /// reported stays on screen when a later refresh fails (spec 0012, AC-8).
    /// - Parameters:
    ///   - info: The catalog the daemon loaded, or `nil` before the first snapshot.
    ///   - error: The error of the last snapshot.
    ///   - unsupported: Whether the daemon does not know the catalog method.
    ///   - runtimeInstallAvailable: Whether the build installs runtimes, which adds the
    ///     digest sentence to the official detail.
    ///   - locale: The locale of the date.
    ///   - calendar: The calendar of the date.
    ///   - timeZone: The time zone of the date.
    /// - Returns: The row. Only a row built from `info` has a toggle.
    static func make(
        info: CatalogInfo?, error: ErrorBody?, unsupported: Bool,
        runtimeInstallAvailable: Bool,
        locale: Locale = .current, calendar: Calendar = .current, timeZone: TimeZone = .current
    ) -> CatalogTrustContent {
        if let info {
            let style = Date.FormatStyle(
                date: .abbreviated, time: .omitted, locale: locale, calendar: calendar,
                timeZone: timeZone)
            let revision = OnboardingCopy.catalogRevision(
                revision: info.revision, published: info.publishedAt.formatted(style),
                outdated: info.outdated)
            if info.trust == .official {
                return CatalogTrustContent(
                    label: OnboardingCopy.catalogLabelOfficial,
                    detail: OnboardingCopy.catalogDetailOfficial
                        + (runtimeInstallAvailable ? OnboardingCopy.catalogDetailDigest : ""),
                    revision: revision, toggle: OnboardingCopy.catalogToggleOfficial)
            }
            if info.source == .development {
                return CatalogTrustContent(
                    label: OnboardingCopy.catalogLabelDevelopment,
                    detail: OnboardingCopy.catalogDetailDevelopment, revision: revision,
                    toggle: OnboardingCopy.catalogToggleDevelopment)
            }
            return CatalogTrustContent(
                label: OnboardingCopy.catalogLabelUnknown,
                detail: OnboardingCopy.catalogDetailUnknown
                    + (info.trust == .unrecognized ? OnboardingCopy.catalogDetailUnknownTrust : ""),
                revision: revision, toggle: OnboardingCopy.catalogToggleUnknown)
        }
        if unsupported {
            return CatalogTrustContent(
                label: OnboardingCopy.catalogLabelPlain,
                detail: OnboardingCopy.catalogDetailUnsupported, revision: nil, toggle: nil)
        }
        if let error {
            return CatalogTrustContent(
                label: OnboardingCopy.catalogLabelPlain,
                detail: OnboardingCopy.catalogDetailError(message: error.message),
                revision: error.hint, toggle: nil)
        }
        return CatalogTrustContent(
            label: OnboardingCopy.catalogLabelPlain, detail: OnboardingCopy.catalogDetailWaiting,
            revision: nil, toggle: nil)
    }
}
