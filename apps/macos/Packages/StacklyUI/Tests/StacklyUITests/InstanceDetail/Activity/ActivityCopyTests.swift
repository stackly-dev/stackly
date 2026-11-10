import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The words of the Activity tab (spec 0035, *Teks Activity*, AC-15, AC-18): the Time, By,
/// and Activity column of every action, the fallback of a template whose detail is missing,
/// and a failure that never reads as a success.
struct ActivityCopyTests {
    private static let utc = TimeZone(identifier: "UTC") ?? .gmt
    private static let locale = Locale(identifier: "en_US")
    private static let calendar = Calendar(identifier: .gregorian)

    private func text(
        _ action: String = "operation.finished", kind: OperationKind? = .startInstance,
        result: AuditResultKind = .succeeded, configRevision: UInt64? = nil,
        details: [String: String] = [:]
    ) -> String {
        ActivityCopy.activity(
            ActivitySample.row(
                action: action, kind: kind, result: result, configRevision: configRevision,
                details: details))
    }

    @Test func theActivityTabComesAfterMetrics() {
        #expect(
            InstanceTab.allCases.map(\.title)
                == ["Summary", "Configuration", "Logs", "Metrics", "Activity"])
    }

    // MARK: - By

    @Test func theByColumnNamesTheClientThenStacklyThenClient() {
        #expect(ActivityCopy.by(ActivitySample.row(client: .gui)) == "App")
        #expect(ActivityCopy.by(ActivitySample.row(client: .cli)) == "CLI")
        #expect(ActivityCopy.by(ActivitySample.row(actor: .daemon, client: nil)) == "Stackly")
        #expect(ActivityCopy.by(ActivitySample.row(actor: .user, client: nil)) == "Client")
        #expect(
            ActivityCopy.by(ActivitySample.row(actor: .user, client: .unrecognized)) == "Client")
        #expect(ActivityCopy.by(ActivitySample.row(actor: .unrecognized, client: nil)) == "Client")
    }

    // MARK: - Time

    @Test func aMomentOfTodayShowsTheShortTimeAndAnotherDayTheShortDate() throws {
        let now = try #require(ISO8601DateFormatter().date(from: "2026-11-10T15:00:00Z"))
        let today = try #require(ISO8601DateFormatter().date(from: "2026-11-10T09:05:00Z"))
        let earlier = try #require(ISO8601DateFormatter().date(from: "2026-11-08T23:59:00Z"))
        let lastYear = try #require(ISO8601DateFormatter().date(from: "2025-12-31T10:00:00Z"))
        func time(_ date: Date) -> String {
            ActivityCopy.time(
                date, now: now, calendar: Self.calendar, locale: Self.locale,
                timeZone: Self.utc)
        }
        #expect(time(today) == "9:05\u{202F}AM")
        #expect(time(earlier) == "Nov 8")
        #expect(time(lastYear) == "Dec 31, 2025")
        #expect(
            ActivityCopy.fullTime(
                today, calendar: Self.calendar, locale: Self.locale, timeZone: Self.utc)
                == "November 10, 2026 at 9:05:00\u{202F}AM")
    }

    @Test func todayFollowsTheTimeZoneOfTheWindow() throws {
        let now = try #require(ISO8601DateFormatter().date(from: "2026-11-10T01:00:00Z"))
        let lateYesterdayInUTC = try #require(
            ISO8601DateFormatter().date(from: "2026-11-09T23:00:00Z"))
        let tokyo = try #require(TimeZone(identifier: "Asia/Tokyo"))
        #expect(
            ActivityCopy.time(
                lateYesterdayInUTC, now: now, calendar: Self.calendar, locale: Self.locale,
                timeZone: Self.utc) == "Nov 9")
        #expect(
            ActivityCopy.time(
                lateYesterdayInUTC, now: now, calendar: Self.calendar, locale: Self.locale,
                timeZone: tokyo) == "8:00\u{202F}AM")
    }

    // MARK: - Operations

    @Test func aCreateSaysWhereItCameFromWhenItWasADuplicate() {
        #expect(text(kind: .createInstance) == "Created")
        #expect(
            text(
                kind: .createInstance,
                details: [
                    "duplicateOf": ActivitySample.instance.uuidString,
                    "duplicateOfName": "pg17-dev",
                ])
                == "Created from pg17-dev")
        #expect(
            text(
                kind: .createInstance, result: .failed,
                details: ["duplicateOfName": "pg17-dev", "errorCode": "supervisor.init_failed"])
                == "Create failed · supervisor.init_failed")
    }

    @Test func theLifecycleKindsReadAsWhatHappened() {
        #expect(text(kind: .startInstance) == "Started")
        #expect(text(kind: .stopInstance) == "Stopped")
        #expect(text(kind: .restartInstance) == "Restarted")
        #expect(text(kind: .reloadInstance) == "Reloaded")
        #expect(text(kind: .forceStopInstance) == "Force stopped")
    }

    @Test func aFailureNamesTheVerbAndTheErrorCodeAndACancelTheVerb() {
        #expect(
            text(
                kind: .startInstance, result: .failed,
                details: ["errorCode": "supervisor.endpoint_in_use", "reason": "step_failed"])
                == "Start failed · supervisor.endpoint_in_use")
        #expect(text(kind: .stopInstance, result: .failed) == "Stop failed")
        #expect(text(kind: .restartInstance, result: .cancelled) == "Restart cancelled")
        #expect(
            text(
                kind: .startInstance, result: .failed,
                details: ["errorCode": "operation.abandoned", "reason": "abandoned"])
                == "Start abandoned · operation.abandoned")
    }

    @Test func aPortChangeListsThePortsOrFallsBack() {
        #expect(
            text(kind: .changeEndpoints, configRevision: 3, details: ["ports": "main 5432 → 5433"])
                == "Ports changed: main 5432 → 5433")
        #expect(text(kind: .changeEndpoints) == "Ports changed")
        #expect(
            text(
                kind: .changeEndpoints, result: .failed,
                details: ["errorCode": "config.revision_conflict", "ports": "main 5432 → 5433"])
                == "Change ports failed · config.revision_conflict")
    }

    @Test func aConfigurationChangeNamesItsRevisionOrFallsBack() {
        #expect(
            text(kind: .applyConfig, configRevision: 4) == "Configuration revision 4 applied")
        #expect(text(kind: .applyConfig) == "Configuration applied")
        #expect(
            text(kind: .resolveConfigDrift, configRevision: 2)
                == "Configuration files rewritten from revision 2")
        #expect(text(kind: .resolveConfigDrift) == "Configuration files rewritten")
        #expect(
            text(kind: .applyConfig, result: .failed, configRevision: 4)
                == "Apply configuration failed")
    }

    @Test func aRenameNamesBothNamesOrFallsBack() {
        #expect(
            text(kind: .renameInstance, details: ["newName": "pg-main", "oldName": "pg17-dev"])
                == "Renamed from pg17-dev to pg-main")
        #expect(text(kind: .renameInstance, details: ["oldName": "pg17-dev"]) == "Renamed")
    }

    @Test func anotherKindReadsAsItsTitleWithoutTheInstanceThenTheResult() {
        #expect(text(kind: .backupInstance) == "Back up succeeded")
        #expect(text(kind: .setInstanceStartup, result: .failed) == "Change autostart failed")
        #expect(text(kind: .restoreInstance, result: .cancelled) == "Restore cancelled")
        #expect(text(kind: .unrecognized) == "Operation succeeded")
        #expect(text(kind: nil) == "Operation succeeded")
    }

    @Test func theRecoveryActionsOfAnOperationReadAsTheirOwnRows() {
        #expect(
            text("operation.needs_recovery", kind: .restoreInstance, result: .needsRecovery)
                == "Restore needs recovery")
        #expect(
            text("operation.cancel", details: ["disposition": "requested"]) == "Cancel requested")
        #expect(text("operation.resume") == "Resumed")
        #expect(text("operation.abandon", details: ["leftovers": "3"]) == "Abandoned, 3 items left")
        #expect(text("operation.abandon", details: ["leftovers": "1"]) == "Abandoned, 1 item left")
        #expect(text("operation.abandon") == "Abandoned")
    }

    // MARK: - Secrets

    @Test func aSecretRowNamesTheFieldOfItsReferenceAndNeverAValue() {
        let reference =
            "local:instance/\(ActivitySample.instance.uuidString.lowercased())/superuser-password"
        #expect(
            text("secret.reveal", details: ["purpose": "show", "reference": reference])
                == "Viewed superuser password")
        #expect(
            text("secret.reveal", details: ["purpose": "copy", "reference": reference])
                == "Copied superuser password")
        #expect(
            text("secret.set", details: ["reference": reference]) == "Changed superuser password")
        #expect(
            text("secret.delete", details: ["reference": reference]) == "Deleted superuser password"
        )
        #expect(
            text(
                "secret.reveal", result: .failed,
                details: [
                    "errorCode": "secret.unavailable", "purpose": "show", "reference": reference,
                ]) == "View superuser password failed · secret.unavailable")
        #expect(text("secret.reveal") == "Viewed secret")
    }

    @Test func anActionThisBuildDoesNotKnowShowsAsItIs() {
        #expect(text("backup.verified", kind: nil) == "backup.verified")
    }

    // MARK: - Rows

    @Test func aRowCarriesItsOperationAndMarksAFailure() throws {
        let now = ActivitySample.base.addingTimeInterval(3_600)
        let rows = ActivityCopy.rows(
            [
                ActivitySample.row(2, result: .failed, details: ["errorCode": "x.y"]),
                ActivitySample.row(
                    1, action: "secret.reveal", details: ["purpose": "copy", "reference": "a/b"]),
            ], now: now, calendar: Self.calendar, locale: Self.locale, timeZone: Self.utc)
        #expect(rows.map(\.id) == [ActivitySample.id(2), ActivitySample.id(1)])
        #expect(rows.map(\.isFailure) == [true, false])
        #expect(rows.map(\.operationId) == [ActivitySample.operation(2), nil])
        #expect(rows.map(\.activity) == ["Start failed · x.y", "Copied b"])
        #expect(rows.map(\.by) == ["App", "App"])
    }
}
