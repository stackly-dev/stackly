import Foundation
import Testing

@testable import StacklyUI

/// The Swift twin of `classify` against the rows both languages share (spec 0015, AC-4).
struct AgentClassifierTests {
    /// One row of `protocols/fixtures/agent/states.json`.
    nonisolated struct Row: Decodable, CustomTestStringConvertible, Sendable {
        nonisolated struct Probe: Decodable, Sendable {
            let type: String
            let code: Int?
            let registration: String?
            let raw: Int?
        }
        nonisolated struct Expect: Decodable, Sendable {
            let state: String
            let reason: String?
            let recovery: String
        }
        let name: String
        let rootOverride: Bool
        let probe: Probe
        let launchRecord: Bool
        let justRegistered: Bool
        let expect: Expect

        var testDescription: String { name }

        var input: AgentInput {
            let outcome: ProbeOutcome =
                switch probe.type {
                case "not_checked": .notChecked
                case "unsupported": .unsupported
                case "failed": .failed(code: probe.code)
                case "not_bundled": .probe(.notBundled(executable: "/x/stackly-cli"))
                case "helper_missing": .probe(.helperMissing(bundle: "/x/Stackly.app"))
                default: .probe(.registration(registration, bundle: "/x/Stackly.app"))
                }
            return AgentInput(
                rootOverride: rootOverride, probe: outcome, launchRecord: launchRecord,
                justRegistered: justRegistered)
        }

        private var registration: AgentRegistration {
            switch probe.registration {
            case "enabled": .enabled
            case "requires_approval": .requiresApproval
            case "not_registered": .notRegistered
            case "not_found": .notFound
            default: .unrecognized(probe.raw ?? 0)
            }
        }
    }

    nonisolated struct Table: Decodable {
        let schema: Int
        let rows: [Row]
    }

    nonisolated static let rows: [Row] = {
        var root = URL(filePath: #filePath)
        for _ in 0..<8 { root = root.deletingLastPathComponent() }
        let url = root.appending(path: "protocols/fixtures/agent/states.json")
        guard let data = try? Data(contentsOf: url),
            let table = try? JSONDecoder().decode(Table.self, from: data)
        else {
            return []
        }
        return table.rows
    }()

    @Test func theFixtureHasEveryRowTheRustTestReads() {
        #expect(Self.rows.count == 29)
    }

    @Test(arguments: rows)
    func everyFixtureRowClassifiesAsExpected(row: Row) {
        let assessment = BackgroundAgentClassifier.classify(row.input)
        #expect(assessment.state.rawValue == row.expect.state)
        #expect(assessment.reason?.rawValue == row.expect.reason)
        #expect(assessment.recovery.rawValue == row.expect.recovery)
    }

    @Test func aNeverRegisteredBundleOffersToRegister() {
        for record in [false, true] {
            let assessment = BackgroundAgentClassifier.classify(
                AgentInput(
                    rootOverride: false, probe: .probe(.registration(.notFound, bundle: "/b")),
                    launchRecord: record, justRegistered: false))
            #expect(assessment == AgentAssessment(state: .notRegistered, recovery: .register))
        }
    }

    @Test func notFoundRightAfterRegisteringOffersToMoveOrReinstall() {
        let assessment = BackgroundAgentClassifier.classify(
            AgentInput(
                rootOverride: false, probe: .probe(.registration(.notFound, bundle: "/b")),
                launchRecord: false, justRegistered: true))
        #expect(assessment == AgentAssessment(state: .notFound, recovery: .moveOrReinstallApp))
    }

    @Test func theIdentifiersMatchTheSharedFixture() throws {
        let url = SourceTree.url(of: "protocols/fixtures/bundle/identifiers.json")
        let object = try #require(
            try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: String])
        #expect(AgentIdentifiers.appBundleID == object["appBundleId"])
        #expect(AgentIdentifiers.daemonLabel == object["daemonLabel"])
        #expect(AgentIdentifiers.daemonPlistFile == object["daemonPlistFile"])
        #expect(AgentIdentifiers.daemonSigningID == object["daemonSigningId"])
        #expect(AgentIdentifiers.cliSigningID == object["cliSigningId"])
        #expect(AgentIdentifiers.testAgentLabelPrefix == object["testAgentLabelPrefix"])
        #expect(AgentIdentifiers.appExecutable == object["appExecutable"])
        #expect(AgentIdentifiers.daemonExecutable == object["daemonExecutable"])
        #expect(AgentIdentifiers.cliExecutable == object["cliExecutable"])
        #expect(AgentIdentifiers.macosDir == object["macosDir"])
        #expect(AgentIdentifiers.launchAgentsDir == object["launchAgentsDir"])
        #expect(AgentIdentifiers.resourcesDir == object["resourcesDir"])
        #expect(AgentIdentifiers.daemonBundleProgram == object["daemonBundleProgram"])
    }

    @Test func everyActionErrorHasItsText() {
        #expect(
            AgentActionError.os(
                op: .register, kind: .other, domain: "SMAppServiceErrorDomain", code: 5
            )
            .message == "macOS returned SMAppServiceErrorDomain 5 while trying to register.")
        #expect(
            AgentActionError.os(op: .register, kind: .invalidSignature, domain: "d", code: 3)
                .message == "macOS rejected the signature of this build.")
        #expect(AgentActionError.timedOut(op: .status).message == "macOS didn’t answer in time.")
        #expect(
            AgentActionError.daemonDidNotStart(seconds: 30).message
                == "stacklyd didn’t start within 30 seconds.")
        #expect(
            AgentActionError.registrationNotKept.message == "macOS didn’t keep the registration.")
    }
}
