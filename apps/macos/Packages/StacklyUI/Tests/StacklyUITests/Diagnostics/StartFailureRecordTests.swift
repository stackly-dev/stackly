import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The start failure record and `binary_replaced` of the Background Agent section (spec 0034,
/// AC-11): what the reader trusts, and when each fact shows. Every file lives in a temporary
/// folder, never the real Application Support.
struct StartFailureRecordTests {
    private let at = "2026-11-10T08:00:00.000Z"
    private var atDate: Date { Date(timeIntervalSince1970: 1_794_297_600) }

    private func record(_ root: TemporaryFolder, mode: Int16 = 0o600, text: String? = nil) throws
        -> URL
    {
        let file = root.url.appending(path: "daemon-start-failure.json")
        let json =
            text
            ?? #"{"schema": 1, "at": ""# + at
            + #"", "exitCode": 1, "code": "storage.root_not_private", "message": "the data root is not private", "version": "0.9.0", "launch": "launchd", "bundlePath": "/Applications/Stackly.app"}"#
        try Data(json.utf8).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: mode], ofItemAtPath: file.path)
        return file
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected: "A private regular file of schema 1 gives its time, code, and message"
        ))
    func aPrivateRecordIsRead() throws {
        let root = try TemporaryFolder()
        let file = try record(root)
        let read = try #require(StartFailureRecordReader.read(at: file, owner: getuid()))
        #expect(read.at == atDate)
        #expect(read.code == "storage.root_not_private")
        #expect(read.message == "the data root is not private")
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "A record looser than 0600, a symlink, one of another owner, one over 4 KiB, and one that does not parse count as missing"
        ))
    func anUntrustedRecordCountsAsMissing() throws {
        let root = try TemporaryFolder()
        let file = try record(root, mode: 0o644)
        #expect(StartFailureRecordReader.read(at: file, owner: getuid()) == nil)

        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        #expect(StartFailureRecordReader.read(at: file, owner: getuid() &+ 1) == nil)

        let link = root.url.appending(path: "link.json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        #expect(StartFailureRecordReader.read(at: link, owner: getuid()) == nil)

        let big = try record(
            root, text: #"{"schema": 1, "pad": ""# + String(repeating: "x", count: 4100) + #""}"#)
        #expect(StartFailureRecordReader.read(at: big, owner: getuid()) == nil)

        for text in ["not json", #"{"schema": 2, "at": "\#(at)", "code": "c", "message": "m"}"#] {
            let bad = try record(root, text: text)
            #expect(StartFailureRecordReader.read(at: bad, owner: getuid()) == nil)
        }
        #expect(
            StartFailureRecordReader.read(
                at: root.url.appending(path: "none.json"), owner: getuid()) == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "The record shows only while the daemon is unreachable and when it is not older than the daemon that ran last"
        ))
    func theRecordShowsOnlyWhileTheDaemonIsAway() {
        let record = StartFailureRecord(at: atDate, code: "c", message: "m")
        func facts(unreachable: Bool, startedAt: Date?) -> AgentStartFacts {
            AgentStartFacts.resolve(
                record: record, daemonUnreachable: unreachable, daemonStartedAt: startedAt,
                agentEnabled: true, lastLaunchedAt: nil, binaryModifiedAt: nil)
        }
        #expect(facts(unreachable: false, startedAt: nil) == .none)
        #expect(facts(unreachable: true, startedAt: nil).startFailure == record)
        #expect(
            facts(unreachable: true, startedAt: atDate.addingTimeInterval(-60)).startFailure
                == record)
        #expect(
            facts(unreachable: true, startedAt: atDate.addingTimeInterval(60)).startFailure == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "binary_replaced needs an enabled agent, a launch, a binary more than 2 seconds younger, and no start failure newer than the launch"
        ))
    func binaryReplacedFollowsItsRules() {
        let launched = atDate
        func replaced(
            enabled: Bool = true, launchedAt: Date? = launched, binaryAfter: TimeInterval? = 3,
            record: StartFailureRecord? = nil
        ) -> Bool {
            AgentStartFacts.resolve(
                record: record, daemonUnreachable: true, daemonStartedAt: nil,
                agentEnabled: enabled, lastLaunchedAt: launchedAt,
                binaryModifiedAt: binaryAfter.map { launched.addingTimeInterval($0) }
            ).binaryReplaced
        }
        #expect(replaced())
        #expect(replaced(binaryAfter: 2) == false)
        #expect(replaced(enabled: false) == false)
        #expect(replaced(launchedAt: nil) == false)
        #expect(replaced(binaryAfter: nil) == false)
        let newer = StartFailureRecord(at: launched.addingTimeInterval(1), code: "c", message: "m")
        #expect(replaced(record: newer) == false)
        let older = StartFailureRecord(at: launched.addingTimeInterval(-1), code: "c", message: "m")
        #expect(replaced(record: older))
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "The section adds What to do for binary_replaced after State and Last start failure with the code, time, and message after the rows"
        ))
    func theSectionAddsItsRows() {
        let base = [
            DiagnosticsRow("State", text: "On"), DiagnosticsRow("Last error", text: "None"),
        ]
        let utc = TimeZone(identifier: "UTC") ?? .current
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = utc
        let rows = BackgroundAgentDiagnosticsSection.rows(
            base,
            adding: AgentStartFacts(
                startFailure: StartFailureRecord(
                    at: atDate, code: "storage.root_not_private", message: "not private"),
                binaryReplaced: true),
            calendar: calendar, locale: Locale(identifier: "en_US"), timeZone: utc)
        #expect(rows.map(\.label) == ["State", "What to do", "Last error", "Last start failure"])
        #expect(rows[1].text == BackgroundAgentDiagnosticsSection.binaryReplacedStep)
        #expect(rows[3].code == "storage.root_not_private")
        #expect(rows[3].text?.hasSuffix(" · not private") == true)
        #expect(
            BackgroundAgentDiagnosticsSection.rows(
                base, adding: .none, calendar: calendar, locale: Locale(identifier: "en_US"),
                timeZone: utc) == base)
    }
}
