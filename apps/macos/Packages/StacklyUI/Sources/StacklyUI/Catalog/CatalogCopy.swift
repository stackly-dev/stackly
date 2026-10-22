import Foundation
import StacklyClient

/// The words of the Catalog screen (spec 0017, *Copy* and *Nilai asing*): names of OSes,
/// architectures, categories, and capabilities, the text of every reason, sizes, and the
/// toolbar subtitle.
///
/// The daemon decides status, reason, and install verdict; this type only says them. A
/// value a newer daemon sends and this build does not know is written as neutral text,
/// never guessed into another value.
nonisolated public enum CatalogCopy {
    // MARK: - Names

    /// The name of an OS: macOS, Linux, Windows.
    public static func osName(_ os: TargetOs) -> String {
        switch os {
        case .macos: "macOS"
        case .linux: "Linux"
        case .windows: "Windows"
        case .unrecognized: "Unknown"
        }
    }

    /// The name of an architecture on `os`: "Apple Silicon" and "Intel" on macOS, "ARM64"
    /// and "x86 64" elsewhere.
    public static func archName(_ arch: TargetArch, on os: TargetOs) -> String {
        switch (arch, os) {
        case (.aarch64, .macos): "Apple Silicon"
        case (.aarch64, _): "ARM64"
        case (.x8664, .macos): "Intel"
        case (.x8664, _): "x86 64"
        case (.unrecognized, _): "Unknown"
        }
    }

    /// The category in a card's meta line and a detail header: "Database", "Messaging".
    /// A category this build does not know, and a missing one, is "Other".
    public static func categoryName(_ category: ProviderCategory?) -> String {
        switch category {
        case .database: "Database"
        case .messaging: "Messaging"
        case .analytics: "Analytics"
        case .cache: "Cache"
        case .proxy: "Proxy"
        case .testing: "Testing"
        case .unrecognized, nil: "Other"
        }
    }

    /// The label of a capability.
    public static func capabilityLabel(_ capability: ProviderCapability) -> String {
        switch capability {
        case .multiInstance: "Multiple instances"
        case .persistence: "Persistent data"
        case .reload: "Reload without restart"
        case .metrics: "Metrics"
        case .tls: "TLS"
        case .coldBackup: "Cold backup"
        case .onlineBackup: "Online backup"
        case .logicalBackup: "Logical backup"
        case .unrecognized: "Unknown capability"
        }
    }

    /// The three rows every provider has: lifecycle, the readiness probe, and logs.
    public static let baselineCapabilities = ["Lifecycle", "Readiness probe", "Logs"]

    // MARK: - Reasons

    /// The sentence behind a status or a verdict.
    ///
    /// - Parameters:
    ///   - reason: The reason the daemon sent.
    ///   - os: The OS the reason is about, for the "No {OS} build" sentence and the names
    ///     of architectures.
    ///   - arches: The architectures a build exists for, for `no_build_for_arch`.
    ///   - note: The note that came with the status: the platform note, or the reason the
    ///     artifact is blocked.
    public static func reasonText(
        _ reason: SupportReason,
        os: TargetOs = .macos,
        arches: [TargetArch] = [],
        note: String? = nil
    ) -> String {
        switch reason {
        case .untested:
            "Not tested on this Mac yet."
        case .noBuild:
            "No \(osName(os)) build in this catalog."
        case .noBuildForArch:
            noBuildForArch(os: os, arches: arches)
        case .noBuildForHost:
            "No build for macOS on Apple Silicon. Stackly can’t install it on this Mac."
        case .platformNote:
            note ?? ""
        case .artifactBlocked:
            note.map { "Blocked: \($0)" } ?? "Blocked."
        case .fixtureOnly:
            "Test fixture, not installable."
        case .providerPlanned:
            "Not in this release."
        case .packageInvalid:
            "The provider package failed validation."
        case .unrecognized:
            "Reason not recognized by this version of Stackly."
        }
    }

    private static func noBuildForArch(os: TargetOs, arches: [TargetArch]) -> String {
        let host = archName(.aarch64, on: os)
        guard arches.isEmpty == false else { return "No build for \(host)." }
        let list = arches.map { archName($0, on: os) }.joined(separator: ", ")
        return "No build for \(host). Builds exist for \(list)."
    }

    /// The sentence behind one cell of the version matrix, or `nil` when the cell has no
    /// reason.
    public static func reasonText(for cell: TargetSupport) -> String? {
        cell.reason.map {
            reasonText($0, os: cell.os, arches: cell.arches ?? [], note: cell.note)
        }
    }

    /// What VoiceOver reads for a matrix cell: "{OS}, {status}: {reason}", or "{OS},
    /// {status}" without a reason.
    public static func spokenText(for cell: TargetSupport) -> String {
        let head = "\(osName(cell.os)), \(CatalogAppearance.appearance(of: cell.status).label)"
        return reasonText(for: cell).map { "\(head): \($0)" } ?? head
    }

    // MARK: - Trust and source

    /// The Trust row of the detail panel.
    public static func trustText(_ trust: TrustLevel) -> String {
        switch trust {
        case .official: "Official, bundled with Stackly"
        case .development: "Development catalog"
        case .unrecognized: "Unknown trust"
        }
    }

    /// The first part of the toolbar subtitle: "Official" or "Development catalog".
    public static func trustTitle(_ trust: TrustLevel) -> String {
        switch trust {
        case .official: "Official"
        case .development: "Development catalog"
        case .unrecognized: "Unknown trust"
        }
    }

    /// The name of the catalog's source in the stale note: "the bundled catalog" or "the
    /// development catalog".
    public static func sourceName(_ source: CatalogSourceKind) -> String {
        switch source {
        case .bundled: "the bundled catalog"
        case .development: "the development catalog"
        case .unrecognized: "Unknown source"
        }
    }

    /// The toolbar subtitle: "Official · updated Oct 20, 2026".
    ///
    /// It always says when the catalog was published and never claims it is the newest.
    ///
    /// - Parameters:
    ///   - info: The catalog the daemon loaded.
    ///   - locale: The locale of the date.
    ///   - calendar: The calendar of the date.
    ///   - timeZone: The time zone of the date.
    public static func subtitle(
        info: CatalogInfo,
        locale: Locale = .current,
        calendar: Calendar = .current,
        timeZone: TimeZone = .current
    ) -> String {
        let style = Date.FormatStyle(
            date: .abbreviated, time: .omitted, locale: locale, calendar: calendar,
            timeZone: timeZone)
        return "\(trustTitle(info.trust)) · updated \(info.publishedAt.formatted(style))"
    }

    // MARK: - Sizes

    /// A byte count the way the CLI writes it: decimal SI, "6.8 MB", "48.1 MB", "1.2 GB".
    /// It is the `.file` style of `ByteCountFormatter`, in the digits of `locale`.
    ///
    /// - Parameters:
    ///   - bytes: The size in bytes.
    ///   - locale: The locale of the digits.
    public static func sizeText(_ bytes: UInt64, locale: Locale = .current) -> String {
        Int64(clamping: bytes).formatted(.byteCount(style: .file).locale(locale))
    }

    // MARK: - Planned entries

    /// The note of a planned entry when the catalog gave none.
    public static let plannedNote =
        "This provider isn’t available yet. It will appear here once packaging and tests pass."

    /// The meta line of a card, under the name.
    ///
    /// - Parameter provider: The provider.
    public static func metaLine(of provider: ProviderSummary) -> String {
        switch provider.state {
        case .planned: "Not in this release"
        case .invalid: "Failed validation"
        case .available, .blocked, .unsupported, .unrecognized:
            [categoryName(provider.category), provider.licenses?.first]
                .compactMap { $0 }
                .joined(separator: " · ")
        }
    }
}
