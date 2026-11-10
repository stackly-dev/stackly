import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The rows, buttons, focus, and texts of the Doctor section (spec 0034, AC-17, AC-18, AC-20),
/// against the `doctor.run` contract fixtures.
struct DoctorPresentationTests {
    private func rows(
        _ report: DoctorReport, focus: DiagnosticsFocus? = nil, routes: DoctorRoutes = .thisBuild
    ) -> [DoctorRow] {
        DoctorPresentation.rows(report: report, focus: focus, routes: routes)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "The mixed report gives one row per finding and one passed row per check without findings, in the daemon's order, plus a row for the findings a check left out"
        ))
    func rowsFollowTheDaemonOrder() throws {
        let rows = rows(try DoctorFixture.mixed())
        #expect(
            rows.map(\.title) == [
                "stacklyd", "Registry", "Permissions", "Data volume is filling",
                "Port conflict · mysql-legacy", "PID reused · pg-main", "Start failed · pg-main",
                "Scratch folder left behind", "Secrets check unavailable", "2 more in Secrets",
            ])
        #expect(
            rows.map(\.status) == [
                .passed, .passed, .passed, .warning, .failed, .failed, .failed, .warning,
                .warning, .warning,
            ])
        #expect(rows[0].detail == "stacklyd 0.9.0 · protocol 3.4 · launchd")
        #expect(rows[0].finding == nil)
        #expect(Set(rows.map(\.id)).count == rows.count)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "A finding with evidence, an excerpt, or a remedy gets Details…; any other gets its route when this build has it, and none for open_storage_settings or no action"
        ))
    func eachFindingHasAtMostOneButton() throws {
        let mysqlLegacy = try DoctorFixture.mysqlLegacy
        let byTitle = Dictionary(
            uniqueKeysWithValues: rows(try DoctorFixture.mixed()).map { ($0.title, $0.button) })
        #expect(byTitle["Data volume is filling"] == .some(nil))
        #expect(
            byTitle["Port conflict · mysql-legacy"]
                == .route(
                    .changePort(instance: mysqlLegacy, endpoint: "mysql", port: 3308))
        )
        #expect(byTitle["PID reused · pg-main"] == .details)
        #expect(byTitle["Start failed · pg-main"] == .details)
        #expect(byTitle["Scratch folder left behind"] == .some(nil))

        let withSettings = DoctorRoutes(
            available: DoctorRoutes.thisBuild.available.union([.openStorageSettings]))
        let filling = rows(try DoctorFixture.mixed(), routes: withSettings)
            .first { $0.title == "Data volume is filling" }
        #expect(filling?.button == .route(.openStorageSettings))
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "A severity and an action of a newer daemon show as unrecognized with no button"
        ))
    func valuesOfANewerDaemonShowWithoutAButton() throws {
        let rows = rows(try DoctorFixture.unknownValues())
        let something = try #require(rows.first { $0.title == "Something new" })
        #expect(something.status == .unrecognized)
        // It has a remedy, so Details… opens the sheet, whose primary is Done.
        #expect(something.button == .details)
        let action = try #require(something.finding?.action)
        #expect(DoctorActionRoute(action) == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected:
                "A focus on an instance keeps its findings and the global Errors, a focus on a check keeps that check, and a focus with nothing left shows No issues"
        ))
    func aFocusFiltersTheRows() throws {
        let report = try DoctorFixture.mixed()
        let pgMain = try DoctorFixture.pgMain
        let mysqlLegacy = try DoctorFixture.mysqlLegacy
        #expect(
            rows(report, focus: .instance(pgMain)).map(\.title) == [
                "PID reused · pg-main", "Start failed · pg-main",
            ])
        #expect(
            rows(report, focus: .instance(mysqlLegacy)).map(\.title) == [
                "Port conflict · mysql-legacy"
            ])
        #expect(rows(report, focus: .check(.diskSpace)).map(\.title) == ["Data volume is filling"])
        #expect(rows(report, focus: .check(.registry)).map(\.status) == [.passed])

        let other = rows(report, focus: .instance(UUID()))
        #expect(other.map(\.title) == ["No issues"])
        #expect(other.map(\.status) == [.passed])
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "The subtitle says Not run yet, No issues, 1 issue, or the count of Errors and Warnings; the focus label names the instance or the check"
        ))
    func theTextsFollowTheReport() throws {
        let report = try DoctorFixture.mixed()
        let pgMain = try DoctorFixture.pgMain
        #expect(DoctorPresentation.subtitle(nil) == "Not run yet")
        #expect(DoctorPresentation.subtitle(report) == "8 issues")
        #expect(
            DoctorPresentation.subtitle(try DoctorFixture.clean(finishedAt: .now)) == "No issues")
        #expect(
            DoctorPresentation.focusLabel(.instance(pgMain), report: report)
                == "Showing “pg-main”")
        #expect(
            DoctorPresentation.focusLabel(.check(.diskSpace), report: report)
                == "Showing Disk space")
        #expect(
            DoctorCopy.meta(lastRun: "today at 08:00", issues: 8, connected: true)
                == "Last run today at 08:00 · 8 issues")
        #expect(
            DoctorCopy.meta(lastRun: "today at 08:00", issues: 8, connected: false)
                == "Last run today at 08:00 · stacklyd isn’t connected")
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "Every action kind maps to its label and place, and an action without what its route needs has no route"
        ))
    func actionsMapToTheirRoutes() {
        let id = UUID()
        func action(
            _ kind: DoctorActionKind, instance: UUID? = nil, operation: UUID? = nil,
            endpoint: String? = nil, port: UInt16? = nil
        ) -> DoctorAction {
            DoctorAction(
                kind: kind, instanceId: instance, operationId: operation, endpoint: endpoint,
                port: port)
        }
        #expect(DoctorActionRoute(action(.showInstance, instance: id))?.title == "Show Instance")
        #expect(DoctorActionRoute(action(.reviewConfig, instance: id))?.title == "Review")
        #expect(
            DoctorActionRoute(action(.changePort, instance: id, endpoint: "db", port: 5433))?
                .title == "Use Port 5433")
        #expect(DoctorActionRoute(action(.showOperation, operation: id))?.title == "Show Operation")
        #expect(DoctorActionRoute(action(.openStorageSettings))?.title == "Open Settings")

        #expect(DoctorActionRoute(action(.showInstance)) == nil)
        #expect(DoctorActionRoute(action(.changePort, instance: id, endpoint: "db")) == nil)
        #expect(DoctorActionRoute(action(.showOperation, instance: id)) == nil)
        #expect(DoctorRoutes.thisBuild.contains(.openStorageSettings) == false)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "Copy Report copies every check and finding as plain text with the totals and the run ID"
        ))
    func copyReportIsPlainText() throws {
        let text = DoctorReportText.plain(try DoctorFixture.mixed())
        #expect(text.hasPrefix("stacklyd: OK · stacklyd 0.9.0 · protocol 3.4 · launchd\n"))
        #expect(text.contains("  ERROR Port conflict · mysql-legacy\n"))
        #expect(text.contains("  and 2 more"))
        #expect(
            text.hasSuffix(
                "3 errors, 5 warnings · 9 checks · run 0192f3a7-0000-7000-8000-0000000000e1"))
        #expect(text.contains("\u{1B}") == false)
    }
}
