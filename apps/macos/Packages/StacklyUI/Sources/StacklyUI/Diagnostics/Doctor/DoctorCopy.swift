import Foundation
import StacklyClient

/// The English text of the Doctor section and the bundle sheet (spec 0034, *GUI*). The titles
/// and explanations of checks and findings come from the daemon.
nonisolated enum DoctorCopy {
    static let sectionTitle = "Doctor"
    static let runDoctor = "Run Doctor"
    static let running = "Running…"
    static let runningMeta = "Checking permissions, runtimes, endpoints, locks, disk and registry…"
    static let details = "Details…"
    static let showAll = "Show All"
    static let copyReport = "Copy Report"
    static let failedTitle = "Couldn’t Run the Doctor"
    static let unsupportedTitle = "Doctor Unavailable"
    static let unsupportedReason = "Update stacklyd to run the doctor."
    static let disconnectedNoReport = "Connect to stacklyd to run the doctor."
    static let notRunTitle = "The Doctor Hasn’t Run Yet"
    static let notRunMessage =
        "The doctor checks permissions, runtimes, endpoints, locks, disk space, and the registry. It never changes anything."

    /// `Last run {time} · {n} issues`, or `Last run {time} · stacklyd isn’t connected`.
    static func meta(lastRun: String, issues: UInt64?, connected: Bool) -> String {
        guard connected else { return "Last run \(lastRun) · stacklyd isn’t connected" }
        switch issues ?? 0 {
        case 0: return "Last run \(lastRun) · No issues"
        case 1: return "Last run \(lastRun) · 1 issue"
        case let count: return "Last run \(lastRun) · \(count) issues"
        }
    }

    /// When the last run finished, as the meta line says it: `today at 10:23 AM` on the day
    /// of `now`, otherwise the date and time of the Diagnostics page.
    static func lastRun(
        _ date: Date, now: Date, calendar: Calendar, locale: Locale, timeZone: TimeZone
    ) -> String {
        var calendar = calendar
        calendar.timeZone = timeZone
        guard calendar.isDate(date, inSameDayAs: now) else {
            return DiagnosticsTime.format(
                date, calendar: calendar, locale: locale, timeZone: timeZone)
        }
        let time = date.formatted(
            Date.FormatStyle(
                date: .omitted, time: .shortened, locale: locale, calendar: calendar,
                timeZone: timeZone))
        return "today at \(time)"
    }

    // The detail sheet.
    static let evidence = "Evidence"
    static let recordedProcess = "Recorded process"
    static let currentProcess = "Current process"
    static let lock = "Lock"
    static let suspended = "Suspended"
    static let observed = "Observed"
    static let excerpt = "Last output"
    static let remedySubtitle = "From the provider"
    static let pidFile = "PID file"
    static let done = "Done"

    /// The name of an identity check, as the sheet lists it.
    static func checkName(_ name: IdentityCheckName) -> String {
        switch name {
        case .bootMatches: "Same boot"
        case .pidExists: "PID exists"
        case .startMatches: "Start time matches"
        case .executableMatches: "Executable matches"
        case .userMatches: "User matches"
        case .sessionMatches: "Session matches"
        case .holdsLock: "Holds the instance lock"
        case .unrecognized: "Another check"
        }
    }

    /// The result of an identity check.
    static func checkResult(_ result: IdentityCheckResult) -> String {
        switch result {
        case .passed: "Passed"
        case .failed: "Failed"
        case .notChecked: "Not checked"
        case .unrecognized: "Unknown"
        }
    }

    /// A process in one line: `PID 812 · /usr/bin/mysqld`, or the PID of another user.
    static func process(_ process: ProcessRef?) -> String {
        guard let process else { return "None" }
        if process.sameUser == false { return "PID \(process.pid) · another user" }
        guard let executable = process.executable else { return "PID \(process.pid)" }
        return "PID \(process.pid) · \(executable)"
    }

    /// What the PID of a PID file is.
    static func pidFileState(_ evidence: PidFileEvidence) -> String {
        let pid = evidence.pid.map { "PID \($0)" } ?? "No PID"
        switch evidence.state {
        case .missing: return "The file is gone."
        case .unreadable: return "The file can’t be read."
        case .notRunning: return "\(pid) · no process"
        case .runtimeProcess: return "\(pid) · a process of this instance’s runtime"
        case .otherProcess:
            guard let held = evidence.process else { return "\(pid) · another process" }
            if held.sameUser == false { return "\(pid) · another user" }
            return "\(pid) · \(held.executable ?? "another process")"
        case .unrecognized: return pid
        }
    }

    // The bundle.
    static let bundleSectionTitle = "Diagnostic Bundle"
    static let bundleSectionText =
        "A bundle holds the doctor report, the instance list, recent logs, runtime digests, and system facts. Secrets and connection strings are redacted. The bundle is saved locally and never uploaded."
    static let previewBundle = "Preview Bundle…"
    static let exportBundle = "Export Bundle…"
    static let bundleSheetTitle = "Export Diagnostic Bundle"
    static let export = "Export…"
    static let bundleRedaction =
        "Secrets and connection strings are redacted. Paths in your home folder are shown as ~. Nothing is uploaded."
    static let previewExpired = "The preview expired. Review the new one before you export."
    static let preparing = "Preparing the bundle…"
    static let bundleFailedTitle = "Couldn’t Prepare the Bundle"
    static let exportFailedTitle = "Couldn’t Export the Bundle"
    static let tryAgain = "Try Again"
    static let showInFinder = "Show in Finder"

    /// `Saved {file} to {folder}.`
    static func saved(file: String, folder: String) -> String {
        "Saved \(file) to \(folder)."
    }
}

/// The report as plain text for Copy Report, like `stackly doctor` without color.
nonisolated enum DoctorReportText {
    /// The text of `report`.
    static func plain(_ report: DoctorReport) -> String {
        var lines: [String] = []
        for check in report.checks ?? [] {
            lines.append("\(check.title): \(label(check.severity)) · \(check.summary)")
            for finding in check.findings ?? [] {
                lines.append("  \(label(finding.severity)) \(finding.title)")
                lines.append("    \(finding.explanation)")
            }
            if check.omitted > 0 {
                lines.append("  and \(check.omitted) more")
            }
        }
        let checks = report.checks?.count ?? 0
        lines.append("")
        lines.append(
            "\(report.errors) errors, \(report.warnings) warnings · \(checks) checks · run \(report.runId.uuidString.lowercased())"
        )
        return lines.joined(separator: "\n")
    }

    private static func label(_ severity: DoctorSeverity) -> String {
        switch severity {
        case .error: "ERROR"
        case .warning: "WARNING"
        case .ok: "OK"
        case .unrecognized: "UNKNOWN"
        }
    }
}
