import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The fixture catalog of spec 0068 (AC-1, AC-2): names, IDs, DTO round trips, and the
/// status the shell derives from each case.
struct FixtureScenarioTests {
    /// The scenario names of the spec's catalog table, in order.
    nonisolated static let names = [
        "connected-empty", "connected-mixed", "daemon-degraded", "daemon-health-unrecognized",
        "daemon-refreshing", "daemon-starting", "daemon-stopping", "daemon-connecting",
        "daemon-offline", "daemon-unreachable", "daemon-incompatible-major",
        "daemon-incompatible-rejected", "daemon-untrusted", "daemon-recovery",
        "daemon-misconfigured", "onboarding", "agent-requires-approval", "agent-disabled",
        "agent-enabled", "agent-not-found", "agent-unavailable",
    ]

    @Test func theCatalogHasEveryScenarioOfTheSpecWithItsFixedName() {
        #expect(FixtureScenario.ID.allCases.map(\.rawValue) == Self.names)
        #expect(FixtureScenario.all.map(\.id.rawValue) == Self.names)
        #expect(FixtureScenario(.connectedMixed).id == .connectedMixed)
    }

    @Test func onlyOnboardingHasNoExpectedPresence() {
        for scenario in FixtureScenario.all {
            #expect((scenario.expectedPresence == nil) == (scenario.id == .onboarding))
        }
    }

    @Test func fixtureIDsAreUUIDv7ShapedAndNumberedByKindAndCase() {
        #expect(
            FixtureID.uuid(kind: 1, number: 3).uuidString == "00000000-0000-7000-8000-000000000103")
        #expect(
            FixtureID.uuid(kind: 4, number: 1).uuidString == "00000000-0000-7000-8000-000000000401")
        let instances = FixtureScenario.instanceCases.map(\.id)
        #expect(Set(instances).count == instances.count)
        #expect(instances.first == FixtureID.uuid(kind: FixtureID.instance, number: 1))
    }

    @Test func theClockIsFixedAndCountsBack() {
        #expect(FixtureClock.now == Date(timeIntervalSince1970: 1_792_300_000))
        #expect(FixtureClock.minutesAgo(5).timeIntervalSince(FixtureClock.now) == -300)
        #expect(FixtureClock.daysAgo(2).timeIntervalSince(FixtureClock.now) == -172_800)
    }

    @Test func everyScenarioReportsTheFixtureContractNotTheCurrentOne() {
        #expect(FixtureContract.protocolVersion == ProtocolVersion(major: 1, minor: 8))
        #expect(FixtureContract.protocolText == "1.8")
        #expect(HandshakeAccepted.fixture.protocol == FixtureContract.protocolVersion)
        #expect(HandshakeAccepted.fixture.contractDigest == FixtureContract.digest)
        #expect(DaemonInfo.fixture.protocol == FixtureContract.protocolVersion)
        #expect(DaemonInfo.fixture.contractDigest == FixtureContract.digest)
        for scenario in FixtureScenario.all {
            if let handshake = scenario.handshake {
                #expect(handshake.protocol == FixtureContract.protocolVersion)
            }
            for step in scenario.connection {
                guard case .state(.connected(let daemon)) = step else { continue }
                #expect(daemon.accepted.protocol == FixtureContract.protocolVersion)
                #expect(daemon.accepted.contractDigest == FixtureContract.digest)
                #expect(daemon.info?.protocol == FixtureContract.protocolVersion)
            }
        }
    }

    @Test func pinnedDiagnosticsShowTheFixtureAppProtocol() {
        let live = ConnectionDiagnostics(
            status: .unreachable(lastKnownAt: nil, since: FixtureClock.now), socketPath: nil,
            lastError: nil, nextAttempt: nil,
            lastConnectedAt: nil, daemon: nil, daemonProtocol: nil, appProtocol: "9.9",
            contractDrift: nil, health: nil)
        #expect(live.pinningFixtureContract().appProtocol == FixtureContract.protocolText)
    }

    // MARK: - AC-1: DTOs round trip

    private static func roundTrips<Value: Codable & Equatable>(_ value: Value) throws -> Bool {
        let data = try StacklyJSON.encoder().encode(value)
        return try StacklyJSON.decoder().decode(Value.self, from: data) == value
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Every DTO of every fixture scenario encodes and decodes to itself"),
        arguments: FixtureScenario.all
    )
    func everyDTOOfEveryScenarioRoundTripsThroughStacklyJSON(scenario: FixtureScenario) throws {
        if let handshake = scenario.handshake { #expect(try Self.roundTrips(handshake)) }
        if let info = scenario.daemonInfo { #expect(try Self.roundTrips(info)) }
        if let health = scenario.health { #expect(try Self.roundTrips(health)) }
        for instance in scenario.instances { #expect(try Self.roundTrips(instance)) }
        for operation in scenario.operations { #expect(try Self.roundTrips(operation)) }
        for provider in scenario.providers { #expect(try Self.roundTrips(provider)) }
        for step in scenario.connection {
            switch step {
            case .state(.unreachable(let error, _, _)), .state(.untrusted(let error, _, _)):
                #expect(try Self.roundTrips(error))
            case .state(.incompatible(let reason, _, _)):
                #expect(try Self.roundTrips(reason.error))
            default:
                break
            }
        }
    }

    @Test func theMixedScenarioHoldsEveryCaseAndTheEmptyOnesHoldNone() {
        let mixed = FixtureScenario(.connectedMixed)
        #expect(mixed.instances.count == 11)
        #expect(mixed.operations.count == 4)
        #expect(mixed.providers.count == 3)
        for id in [
            FixtureScenario.ID.connectedEmpty, .daemonStarting, .daemonRecovery, .onboarding,
        ] {
            let scenario = FixtureScenario(id)
            #expect(scenario.instances.isEmpty && scenario.operations.isEmpty)
        }
        for id in [
            FixtureScenario.ID.daemonDegraded, .daemonHealthUnrecognized, .daemonStopping,
            .daemonOffline,
        ] {
            #expect(FixtureScenario(id).instances == mixed.instances)
        }
    }

    @Test func noFixtureCarriesASecretValueOnlyReferences() throws {
        let mixed = FixtureScenario(.connectedMixed)
        let text = String(
            decoding: try StacklyJSON.encoder().encode(mixed.instances), as: UTF8.self
        ).replacing("\\/", with: "/")
        #expect(text.contains("local:instance/"))
        #expect(text.localizedCaseInsensitiveContains("password\":") == false)
        #expect(text.contains(FixtureCredential.prefix) == false)
    }

    // MARK: - AC-2: instance and operation cases

    /// The label, tone, and detail of each instance case of the spec's table.
    nonisolated static let instanceRows: [(Int, String, String, Tone, String?)] = [
        (1, "pg17-dev", "Running", .positive, nil),
        (2, "orders-db", "Starting…", .caution, "Waiting for probe"),
        (3, "cache", "Stopping…", .caution, nil),
        (4, "mysql-erp", "Stopped", .neutral, nil),
        (5, "queue", "Failed", .critical, nil),
        (6, "analytics", "Failed", .critical, nil),
        (7, "legacy-pg", "Stopped", .neutral, nil),
        (8, "ledger", "Unknown", .caution, "Process identity not confirmed"),
        (9, "billing-db", "Stopped", .neutral, nil),
        (10, "search", "Not Ready", .caution, "connection refused"),
        (11, "metrics", "Running", .positive, "Degraded"),
    ]

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Each instance case maps to the label, tone, and detail of its table row"),
        arguments: instanceRows
    )
    func eachInstanceCaseMapsToItsLabelToneAndDetail(
        row: (Int, String, String, Tone, String?)
    ) throws {
        let (number, name, label, tone, detail) = row
        let instance = FixtureScenario.instanceCases[number - 1]
        #expect(instance.name == name)
        #expect(instance.id == FixtureID.uuid(kind: FixtureID.instance, number: UInt8(number)))
        let appearance = StatusAdapter.appearance(of: instance)
        #expect(appearance.label == label)
        #expect(appearance.tone == tone)
        #expect(appearance.detail?.text == detail)
    }

    @Test func theInstanceCasesCarryTheirProvidersVersionsPortsAndBlockers() {
        let cases = FixtureScenario.instanceCases
        #expect(
            cases.map(\.provider.name).prefix(4) == [
                "official/postgresql", "official/postgresql", "official/valkey", "official/mysql",
            ])
        #expect(
            cases.map(\.runtimeVersion) == [
                "17.6", "18.1", "8.1.4", "8.4.6", "2.11.9", "25.8.3", "16.10", "17.6", "17.6",
                "18.1",
                "17.6",
            ])
        #expect(
            cases.compactMap { $0.endpoints?.first?.port } == [
                5432, 5433, 6379, 3306, 4222, 8123, 5434, 5435, 5436, 5437, 5438,
            ])
        #expect(cases[0].endpoints?.first?.bound == true)
        #expect(cases[4].lastFailure?.code == "crash_loop")
        #expect(cases[4].lastFailure?.excerpt?.count == 2)
        #expect(cases[4].restart.attempts == 5 && cases[4].restart.exhausted)
        #expect(cases[5].lastFailure?.exitCode == 1)
        guard case .dataInUse = cases[6].blocker else {
            Issue.record("case 7 must be blocked by data in use")
            return
        }
        guard case .identityUnproven = cases[7].blocker else {
            Issue.record("case 8 must be blocked by an unproven identity")
            return
        }
        guard case .needsSecret(let blocker) = cases[8].blocker else {
            Issue.record("case 9 must need a secret")
            return
        }
        #expect(cases[8].desired == .running)
        #expect(blocker.references?.first?.reference.hasPrefix("local:instance/") == true)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Each operation case maps to the label and tone of its table row"))
    func eachOperationCaseMapsToItsLabelAndTone() {
        let expected: [(String, Tone)] = [
            ("Running", .informative), ("Queued", .neutral), ("Failed", .critical),
            ("Needs Recovery", .critical),
        ]
        let cases = FixtureScenario.operationCases
        #expect(cases.count == expected.count)
        for (operation, row) in zip(cases, expected) {
            let appearance = StatusAdapter.appearance(of: operation)
            #expect(appearance.label == row.0)
            #expect(appearance.tone == row.1)
        }
        #expect(cases[0].progress == .counted(CountedProgress(done: 42, total: 100)))
        #expect(cases[2].failure != nil)
        #expect(cases[3].recovery != nil)
    }

    @Test func theProviderCasesAreAvailablePlannedAndUnsupported() {
        let cases = FixtureScenario.providerCases
        #expect(cases.map(\.state) == [.available, .planned, .unsupported])
        #expect(cases.map(\.stateReason) == [nil, .providerPlanned, .noBuildForHost])
        #expect(cases[0].category == .database)
    }
}
