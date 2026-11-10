import StacklyClient
import SwiftUI

/// The detail of one doctor finding (spec 0034, *GUI*): the explanation, the evidence of the
/// process check, the last output, and the provider's remedy with its commands and paths to
/// copy, then the finding's safe action as the primary button, or Done.
///
/// A remedy is only text: the sheet never runs a command or removes a file.
struct DoctorFindingSheet: View {
    let finding: DoctorFinding
    let routes: DoctorRoutes
    let open: @MainActor (DoctorActionRoute) -> Void

    @Environment(\.dismiss) private var dismiss
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    var body: some View {
        StacklySheet(title: finding.title, primary: primary, minWidth: 560) {
            DoctorFindingDetail(
                finding: finding, calendar: calendar, locale: locale, timeZone: timeZone)
        }
    }

    private var primary: StacklyAction {
        if let action = finding.action, let route = DoctorActionRoute(action),
            routes.contains(route)
        {
            return StacklyAction(
                id: "finding.route", title: route.title, mutates: false,
                perform: {
                    dismiss()
                    open(route)
                })
        }
        return StacklyAction(
            id: "finding.done", title: DoctorCopy.done, mutates: false, perform: { dismiss() })
    }
}

/// The body of the finding sheet, apart from the sheet so a snapshot can render it.
struct DoctorFindingDetail: View {
    let finding: DoctorFinding
    let calendar: Calendar
    let locale: Locale
    let timeZone: TimeZone

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s16) {
            Text(finding.explanation)
                .font(.body)
                .fixedSize(horizontal: false, vertical: true)
            if let evidence = finding.evidence {
                CardSection(DoctorCopy.evidence) {
                    ForEach(evidenceRows(evidence), id: \.label) { row in
                        LabeledContent(row.label, value: row.value)
                    }
                }
                .labeledContentStyle(StacklyLabeledContentStyle())
            }
            if let owner = finding.portOwner, let process = owner.process {
                CardSection(DoctorCopy.evidence) {
                    LabeledContent("Held by", value: DoctorCopy.process(process))
                    if let instance = owner.instance {
                        LabeledContent("Instance", value: instance.name)
                    }
                }
                .labeledContentStyle(StacklyLabeledContentStyle())
            }
            if let excerpt = finding.excerpt, !excerpt.isEmpty {
                VStack(alignment: .leading, spacing: Spacing.s6) {
                    Text(DoctorCopy.excerpt)
                        .font(.headline)
                    ConsoleBlock(
                        text: excerpt.joined(separator: "\n"), copy: ConsoleCopy(what: "output"))
                }
            }
            if let remedy = finding.remedy {
                remedySection(remedy)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func remedySection(_ remedy: DoctorRemedy) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(remedy.title)
                    .font(.headline)
                Text("\(DoctorCopy.remedySubtitle) \(remedy.provider) \(remedy.revision)")
                    .font(.subheadline)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            if let pidFile = remedy.pidFile {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Text("\(DoctorCopy.pidFile): \(pidFile.path)")
                            .font(.callout.monospaced())
                            .textSelection(.enabled)
                        Text(DoctorCopy.pidFileState(pidFile))
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                    Spacer(minLength: Spacing.s8)
                    CopyButton(pidFile.path, what: "path")
                }
            }
            ForEach(Array((remedy.steps ?? []).enumerated()), id: \.offset) { index, step in
                VStack(alignment: .leading, spacing: Spacing.s6) {
                    Text("\(index + 1). \(step.text)")
                        .font(.body)
                        .fixedSize(horizontal: false, vertical: true)
                    if let command = step.command {
                        ConsoleBlock(command: command)
                    }
                    if let path = step.path {
                        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                            Text(path)
                                .font(.callout.monospaced())
                                .textSelection(.enabled)
                            Spacer(minLength: Spacing.s8)
                            CopyButton(path, what: "path")
                        }
                    }
                }
            }
        }
    }

    private struct Row {
        let label: String
        let value: String
    }

    private func evidenceRows(_ evidence: DoctorEvidence) -> [Row] {
        var rows = [
            Row(label: DoctorCopy.recordedProcess, value: DoctorCopy.process(evidence.recorded)),
            Row(label: DoctorCopy.currentProcess, value: DoctorCopy.process(evidence.current)),
        ]
        for check in evidence.identity.checks ?? [] {
            rows.append(
                Row(
                    label: DoctorCopy.checkName(check.name),
                    value: DoctorCopy.checkResult(check.result)))
        }
        let lock: String =
            switch evidence.identity.lock {
            case .free: "Free"
            case .held: "Held"
            case .unrecognized: "Unknown"
            case nil: DoctorCopy.checkResult(.notChecked)
            }
        rows.append(Row(label: DoctorCopy.lock, value: lock))
        rows.append(
            Row(label: DoctorCopy.suspended, value: evidence.identity.stopped ? "Yes" : "No"))
        rows.append(
            Row(
                label: DoctorCopy.observed,
                value: DiagnosticsTime.format(
                    evidence.observedAt, calendar: calendar, locale: locale, timeZone: timeZone)))
        return rows
    }
}
