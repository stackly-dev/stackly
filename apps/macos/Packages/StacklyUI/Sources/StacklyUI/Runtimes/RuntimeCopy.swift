import Foundation
import StacklyClient

/// The words of the Runtimes page and its inspector (spec 0018, *Halaman Runtimes*): the
/// columns of a row, what VoiceOver reads, and the facts of the inspector.
///
/// The daemon decides every state and support verdict; this type only says them. A value a
/// newer daemon sends and this build does not know is written as neutral text.
nonisolated public enum RuntimeCopy {
    /// The note under the table.
    public static let footnote =
        "Runtimes in use can’t be removed, even by stopped instances. External runtimes are read-only and never upgraded by Stackly."
    /// Why Remove is locked while nothing uses the runtime: feature 55 builds removal.
    public static let removeLater = "Removing runtimes arrives in a later build"
    /// The note of an empty page.
    public static let emptyMessage = "Install a runtime from the catalog to create instances."

    // MARK: - Row

    /// "PostgreSQL 17.6".
    public static func name(of runtime: RuntimeSummary) -> String {
        "\(runtime.displayName) \(runtime.version)"
    }

    /// "Install PostgreSQL 18.1": the title of an Installing row and the label of its
    /// progress.
    public static func installTitle(_ install: InstallingRuntime) -> String {
        "Install \(install.displayName) \(install.version)"
    }

    /// The architecture as the CLI writes it: "arm64".
    public static func arch(_ arch: TargetArch) -> String {
        switch arch {
        case .aarch64: "arm64"
        case .x8664: "x86_64"
        case .unrecognized: "unknown"
        }
    }

    /// The second line of a row: "arm64 · verified" for a managed runtime that is
    /// installed, else the label of the state.
    public static func secondLine(of runtime: RuntimeSummary) -> String {
        if runtime.state == .installed, runtime.installMode == .managed {
            return "\(arch(runtime.target.arch)) · verified"
        }
        return RuntimeAppearance.appearance(of: runtime.state).label
    }

    /// The Source column: "Managed", "External", or "Unknown".
    public static func source(_ mode: InstallModeKind) -> String {
        switch mode {
        case .managed: "Managed"
        case .external: "External"
        case .unrecognized: "Unknown"
        }
    }

    /// The Digest column: `sha256:` and the first 8 hex digits, an ellipsis, and the last 4;
    /// a dash for a runtime without a digest.
    public static func shortDigest(_ sha256: String?) -> String {
        guard let sha256, sha256.count > 12 else { return sha256.map { "sha256:\($0)" } ?? "—" }
        return "sha256:\(sha256.prefix(8))…\(sha256.suffix(4))"
    }

    /// The Used By column: the instance names, separated by commas, a retained instance
    /// marked "(retained)", or "Unused".
    public static func usedBy(_ users: [RuntimeUser]?) -> String {
        guard let users, users.isEmpty == false else { return "Unused" }
        return users.map { $0.retained ? "\($0.name) (retained)" : $0.name }
            .joined(separator: ", ")
    }

    /// Why the Remove button is locked: "Used by 2 instances", or, while nothing uses the
    /// runtime, that removal comes later.
    public static func removeReason(of runtime: RuntimeSummary) -> String {
        let count = runtime.usedBy?.count ?? 0
        guard count > 0 else { return removeLater }
        return "Used by \(count) \(count == 1 ? "instance" : "instances")"
    }

    /// What VoiceOver reads for a row: "{name}, managed, installed, verified, used by
    /// {names}".
    public static func spokenLabel(of runtime: RuntimeSummary) -> String {
        var parts = [name(of: runtime), source(runtime.installMode).lowercased()]
        let state = RuntimeAppearance.appearance(of: runtime.state).label
        parts.append(state.lowercased())
        if runtime.state == .installed, runtime.installMode == .managed {
            parts.append("verified")
        }
        let users = usedBy(runtime.usedBy)
        parts.append(users == "Unused" ? "unused" : "used by \(users)")
        return parts.joined(separator: ", ")
    }

    // MARK: - Inspector

    /// The Support row: the status and its reason, such as "Experimental: the runtime ran
    /// on this Mac; the provider is not tested with it yet."
    public static func supportText(of runtime: RuntimeSummary) -> String {
        let status = CatalogAppearance.appearance(of: runtime.support).label
        return "\(status): \(CatalogCopy.reasonText(runtime.supportReason))"
    }

    /// The record that decided the support: "Tested on macOS 27.0 (26A428), arm64, Oct 18,
    /// 2026: launch_agent, ready, stop".
    ///
    /// - Parameters:
    ///   - record: The newest compatibility record.
    ///   - locale: The locale of the date.
    ///   - calendar: The calendar of the date.
    ///   - timeZone: The time zone of the date.
    public static func recordText(
        _ record: CompatibilityRecordView, locale: Locale = .current,
        calendar: Calendar = .current, timeZone: TimeZone = .current
    ) -> String {
        let style = Date.FormatStyle(
            date: .abbreviated, time: .omitted, locale: locale, calendar: calendar,
            timeZone: timeZone)
        var os = "macOS \(record.osVersion)"
        if let build = record.osBuild { os += " (\(build))" }
        let verb = record.result == .pass ? "Tested" : "Tested, failed"
        var text = "\(verb) on \(os), \(arch(record.arch)), \(record.testedAt.formatted(style))"
        if let checks = record.checks, checks.isEmpty == false {
            text += ": " + checks.joined(separator: ", ")
        }
        return text
    }

    /// The Signature row: the kinds of signature the executables carry and how many Mach-O
    /// files the runtime holds, such as "Ad hoc · 210 Mach-O files".
    public static func signatureText(_ signature: SignatureInfo) -> String {
        var kinds: [String] = []
        for executable in signature.executables ?? [] {
            let kind = signatureKind(executable.kind)
            if kinds.contains(kind) == false { kinds.append(kind) }
        }
        if kinds.isEmpty { kinds = [signature.adhocSignedAtInstall ? "Ad hoc" : "Not recorded"] }
        let files = "\(signature.machOFiles) Mach-O \(signature.machOFiles == 1 ? "file" : "files")"
        return "\(kinds.joined(separator: ", ")) · \(files)"
    }

    private static func signatureKind(_ kind: SignatureKindDto) -> String {
        switch kind {
        case .developerId: "Developer ID"
        case .adhoc: "Ad hoc"
        case .linkerSigned: "Linker signed"
        case .other: "Other"
        case .unsigned: "Unsigned"
        case .unrecognized: "Unknown"
        }
    }

    /// One dependency: "libssl.3.dylib 3.5.9 · bundled · Apache-2.0".
    public static func dependencyText(_ dependency: DependencyInfo) -> String {
        var parts = [dependency.version.map { "\(dependency.name) \($0)" } ?? dependency.name]
        parts.append(dependencyKind(dependency.kind))
        if let license = dependency.license { parts.append(license) }
        return parts.joined(separator: " · ")
    }

    private static func dependencyKind(_ kind: DependencyKind) -> String {
        switch kind {
        case .bundled: "bundled"
        case .own: "Stackly runtime"
        case .system: "macOS"
        case .unrecognized: "unknown"
        }
    }

    /// A date for a row of the inspector, in the style of the Catalog subtitle.
    public static func dateText(
        _ date: Date, locale: Locale = .current, calendar: Calendar = .current,
        timeZone: TimeZone = .current
    ) -> String {
        date.formatted(
            Date.FormatStyle(
                date: .abbreviated, time: .shortened, locale: locale, calendar: calendar,
                timeZone: timeZone))
    }
}
