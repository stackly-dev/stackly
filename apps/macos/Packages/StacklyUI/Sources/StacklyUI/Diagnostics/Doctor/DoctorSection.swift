import StacklyClient
import SwiftUI

/// The Doctor section of Diagnostics (spec 0034, AC-17, *State seksi Doctor*): a header with
/// the time of the last run, then one checklist row per finding and one passed row per check
/// without findings, each with at most one button, and Copy Report.
///
/// The report stays while a new run goes and while stacklyd is away, never dimmed (spec
/// 0012). Every button is a place to go or Details…, whose sheet holds the same action;
/// nothing here changes data.
struct DoctorSection: View {
    let doctor: DoctorModel
    let isConnected: Bool
    let focus: DiagnosticsFocus?
    let routes: DoctorRoutes
    let open: @MainActor (DoctorActionRoute, DoctorFinding?) -> Void
    let showAll: @MainActor () -> Void

    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    @State private var detail: FindingDetail?

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                Text(DoctorCopy.sectionTitle)
                    .font(.headline)
                Spacer(minLength: Spacing.s8)
                if let meta {
                    Text(meta)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
            }
            if let focus {
                HStack(spacing: Spacing.s8) {
                    Text(DoctorPresentation.focusLabel(focus, report: doctor.report))
                        .font(.callout)
                    Button(DoctorCopy.showAll, action: showAll)
                        .controlSize(.small)
                }
            }
            content
            if let report = doctor.report {
                CopyButton(title: DoctorCopy.copyReport, what: "report") {
                    DoctorReportText.plain(report)
                }
            }
        }
        .sheet(item: $detail) { item in
            DoctorFindingSheet(
                finding: item.finding, routes: routes,
                open: { route in open(route, item.finding) })
        }
    }

    @ViewBuilder private var content: some View {
        if doctor.unsupported {
            StatePlaceholder(
                UnsupportedContent(
                    title: DoctorCopy.unsupportedTitle, reason: DoctorCopy.unsupportedReason),
                size: .section)
        } else if let report = doctor.report {
            CheckList(items(report), layout: .single)
                .padding(Spacing.s14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .cardSurface()
        } else if doctor.running {
            VStack(spacing: Spacing.s8) {
                DelayedProgress()
                Text(DoctorCopy.runningMeta)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            .frame(maxWidth: .infinity, minHeight: 120)
            .cardSurface()
        } else if !isConnected {
            StatePlaceholder(
                .noData(
                    symbol: "stethoscope", title: DoctorCopy.notRunTitle,
                    message: DoctorCopy.disconnectedNoReport),
                size: .section)
        } else if let failure = doctor.failure {
            StatePlaceholder(
                content: PlaceholderContent(
                    doctorFailure: FailureContent(
                        title: DoctorCopy.failedTitle,
                        message: DaemonText.sentence(failure.message), code: failure.code),
                    retry: StateAction(
                        title: "Retry", mutates: false,
                        perform: { Task { await doctor.request(.user) } })),
                size: .section)
        } else {
            StatePlaceholder(
                .noData(
                    symbol: "stethoscope", title: DoctorCopy.notRunTitle,
                    message: DoctorCopy.notRunMessage,
                    primaryAction: StateAction(
                        title: DoctorCopy.runDoctor, mutates: false,
                        perform: { Task { await doctor.request(.user) } })),
                size: .section)
        }
    }

    /// `Last run …` or the running text, or `nil` before the first report.
    private var meta: String? {
        if doctor.running, doctor.report == nil || isConnected {
            return doctor.report == nil ? nil : DoctorCopy.runningMeta
        }
        guard let report = doctor.report else { return nil }
        let time = DiagnosticsTime.format(
            report.finishedAt, calendar: calendar, locale: locale, timeZone: timeZone)
        return DoctorCopy.meta(
            lastRun: time, issues: DoctorPresentation.issueCount(report), connected: isConnected)
    }

    private func items(_ report: DoctorReport) -> [CheckItem] {
        DoctorPresentation.rows(report: report, focus: focus, routes: routes).map { row in
            CheckItem(
                id: row.id, status: row.status, title: row.title, detail: row.detail,
                action: action(for: row))
        }
    }

    private func action(for row: DoctorRow) -> StacklyAction? {
        switch row.button {
        case .route(let route):
            return StacklyAction(
                id: "doctor.\(row.id)", title: route.title, mutates: false,
                perform: { open(route, row.finding) })
        case .details:
            guard let finding = row.finding else { return nil }
            return StacklyAction(
                id: "doctor.\(row.id).details", title: DoctorCopy.details, mutates: false,
                perform: { detail = FindingDetail(id: row.id, finding: finding) })
        case nil:
            return nil
        }
    }
}

/// A finding shown in the detail sheet.
private struct FindingDetail: Identifiable {
    let id: String
    let finding: DoctorFinding
}
