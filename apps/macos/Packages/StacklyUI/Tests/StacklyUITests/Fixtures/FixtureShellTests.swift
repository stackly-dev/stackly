import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The fixture runtime (spec 0068, AC-2, AC-3): every daemon scenario settles in its
/// presence with the banner of the spec 0013 table, reads come from the scenario, and a
/// mutation fails like the real app while it is not connected.
struct FixtureShellTests {
    /// The banner title of each scenario's presence in the spec 0013 table, `nil` for none.
    nonisolated static let bannerTitles: [FixtureScenario.ID: String?] = [
        .connectedEmpty: nil,
        .connectedMixed: nil,
        .daemonDegraded: nil,
        .daemonHealthUnrecognized: nil,
        .daemonRefreshing: nil,
        .daemonStarting: "stacklyd is starting.",
        .daemonStopping: "stacklyd is shutting down.",
        .daemonConnecting: "Connecting to stacklyd…",
        .daemonOffline: "Can’t connect to stacklyd.",
        .daemonUnreachable: "Can’t connect to stacklyd.",
        .daemonIncompatibleMajor: "This version of Stackly can’t talk to stacklyd.",
        .daemonIncompatibleRejected: "This version of Stackly can’t talk to stacklyd.",
        .daemonUntrusted: "Stackly didn’t connect to stacklyd.",
        .daemonRecovery: "Stackly’s registry needs recovery.",
        .daemonMisconfigured: "STACKLY_ROOT is invalid.",
    ]

    nonisolated static let daemonScenarios = FixtureScenario.all.filter { $0.id != .onboarding }

    @Test func theBannerTableCoversEveryDaemonScenario() {
        #expect(Set(Self.bannerTitles.keys) == Set(Self.daemonScenarios.map(\.id)))
    }

    @Test(
        .evidence(
            covers: ["REQ-024", "AC-024"],
            expected: "Every daemon scenario settles in its presence with the banner of the table"
        ),
        arguments: daemonScenarios
    )
    func everyDaemonScenarioSettlesInItsPresenceAndBanner(scenario: FixtureScenario) async throws {
        let runtime = try await settledRuntime(scenario)
        defer { runtime.willTerminate() }
        let expectedTitle = try #require(Self.bannerTitles[scenario.id])
        #expect(runtime.model.presentation.banner?.title == expectedTitle)
        #expect(runtime.model.presence == scenario.expectedPresence)
    }

    @Test(
        .evidence(
            covers: ["REQ-024", "AC-024"],
            expected: "The offline scenario keeps the last known instances with mutations blocked"
        ))
    func theOfflineScenarioKeepsTheLastKnownStateWithMutationsBlocked() async throws {
        let runtime = try await settledRuntime(FixtureScenario(.daemonOffline))
        defer { runtime.willTerminate() }
        let model = runtime.model
        #expect(model.presence == .unreachable(lastKnownAt: FixtureClock.now))
        #expect(model.instances.instances.count == 11)
        #expect(model.instances.instances.first?.name == "analytics")
        #expect(model.sidebarCounts.instances == 11)
        guard case .blocked(let reason, _) = model.mutationAvailability else {
            Issue.record("mutations must be blocked while offline")
            return
        }
        #expect(reason == "stacklyd is unavailable.")
    }

    @Test func aReadyScenarioAnswersReadsFromItsData() async throws {
        let runtime = try await settledRuntime(FixtureScenario(.connectedMixed))
        defer { runtime.willTerminate() }
        let model = runtime.model
        #expect(model.instances.count == 11)
        #expect(model.operations.active.count == 3)
        #expect(model.sidebarCounts == SidebarCounts(instances: 11, runningOperations: 1))
        #expect(model.mutationAvailability == .available)
        let first = FixtureScenario.instanceCases[0]
        guard
            case .success(.instanceGet(let got)) = await model.read(
                .instanceGet(GetInstanceParams(instanceId: first.id)))
        else {
            Issue.record("instance.get must answer from the scenario")
            return
        }
        #expect(got == first)
        guard
            case .failure(.rejected(let body)) = await model.read(
                .instanceGet(
                    GetInstanceParams(instanceId: FixtureID.uuid(kind: 1, number: 99))))
        else {
            Issue.record("an unknown instance must be rejected")
            return
        }
        #expect(body.code == "resource.not_found")
    }

    @Test func aMutationWithoutAScenarioReplyFailsAsNotConnectedAndNothingIsSent() async throws {
        let runtime = try await settledRuntime(FixtureScenario(.connectedMixed))
        defer { runtime.willTerminate() }
        do {
            _ = try await runtime.model.send(
                .instanceStart(
                    StartInstanceParams(
                        instanceId: FixtureScenario.instanceCases[3].id, expectedRevision: 1)),
                idempotencyKey: "fixture-key")
            Issue.record("the mutation must not be answered")
        } catch {
            guard case .notSent(let body) = error else {
                Issue.record("expected notSent, got \(error)")
                return
            }
            #expect(body.code == "ipc.not_connected")
        }
    }

    @Test func aScenarioReplyAnswersAMutationOfThatMethod() async throws {
        let accepted = InstanceActionAccepted(
            accepted: OperationAccepted(
                operation: .fixture(number: 7, kind: .startInstance), replayed: false),
            alreadyInState: false)
        let base = FixtureScenario(.connectedEmpty)
        let scenario = FixtureScenario(
            id: .connectedEmpty, connection: base.connection, handshake: base.handshake,
            daemonInfo: base.daemonInfo, health: base.health,
            replies: ["instance.start": .reply(.instanceStart(accepted))],
            expectedPresence: base.expectedPresence)
        let runtime = try await settledRuntime(scenario)
        defer { runtime.willTerminate() }
        let reply = try await runtime.model.send(
            .instanceStart(
                StartInstanceParams(
                    instanceId: FixtureID.uuid(kind: 1, number: 1), expectedRevision: nil)),
            idempotencyKey: "fixture-key")
        #expect(reply == .instanceStart(accepted))
    }

    @Test func theRefreshingScenarioNeverFinishesItsSnapshot() async throws {
        let runtime = try await settledRuntime(FixtureScenario(.daemonRefreshing))
        defer { runtime.willTerminate() }
        try await Task.sleep(for: .milliseconds(200))
        #expect(runtime.model.presence == .refreshing)
        #expect(runtime.model.sidebarCounts.instances == nil)
    }

    @Test func aScenarioThatNeverSettlesFailsWithADescriptiveError() async throws {
        let base = FixtureScenario(.connectedEmpty)
        let wrong = FixtureScenario(
            id: .connectedEmpty, connection: base.connection, handshake: base.handshake,
            daemonInfo: base.daemonInfo, health: base.health,
            expectedPresence: .starting)
        await #expect(throws: FixtureSettleError.self) {
            _ = try await settledRuntime(wrong, timeout: .milliseconds(300))
        }
    }

    @Test func theSecretsFixtureNeverRevealsAndFailsUnknownCallsAsNotConnected() async throws {
        let client = SecretsClient.fixture(FixtureScenario(.connectedEmpty))
        do {
            _ = try await client.listSecrets()
            Issue.record("secret.list has no reply in this scenario")
        } catch {
            #expect(error.body.code == "ipc.not_connected")
        }
        do {
            _ = try await client.revealSecret(reference: "local:debug/a", purpose: .copy)
            Issue.record("reveal must never be answered")
        } catch {
            #expect(error.body.code == "ipc.not_connected")
        }
    }
}
