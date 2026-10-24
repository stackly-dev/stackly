import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The Start to Copy journey on the detail page (spec 0021 AC-8, AC-9, AC-18): the status of a
/// process that runs before its probe succeeds, and the connection that becomes available once
/// the port is assigned, with the state of the models free of any secret value.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct InstanceDetailJourneyTests {
    private func instance(readiness state: InstanceReadinessState, reason: String) throws
        -> InstanceSummary
    {
        let base = try DetailFixture.triple("postgresql").instance
        return base.testing(
            readiness: InstanceReadiness(
                state: state, reason: reason, detail: nil, observedAt: base.readiness.observedAt))
    }

    @Test(
        .evidence(
            covers: ["AC-012", "REQ-012", "BR-010"],
            expected:
                "A running process whose probe has not succeeded is never Ready, and becomes Ready when the probe succeeds without reloading the page"
        ))
    func aProcessThatRunsBeforeItsProbeSucceedsIsNeverReady() throws {
        let waiting = try instance(readiness: .notReady, reason: "awaiting_probe")
        let ready = try instance(readiness: .ready, reason: "probe_ok")

        #expect(StatusAdapter.appearance(of: waiting).label == "Starting…")
        #expect(StatusAdapter.appearance(of: waiting).detail?.text == "Waiting for probe")
        #expect(ProcessText.text(for: waiting) == "Running (PID 4001)")
        #expect(ReadinessText.text(for: waiting) == "Waiting for probe")
        #expect(ConnectionCard.isNotAcceptingConnections(waiting))

        #expect(StatusAdapter.appearance(of: ready).label == "Running")
        #expect(ProcessText.text(for: ready) == "Running (PID 4001)")
        #expect(ReadinessText.text(for: ready) == "Ready")
        #expect(ConnectionCard.isNotAcceptingConnections(ready) == false)
    }

    @Test func aProbeThatFailsIsNotReadyWithItsDetailNotReady() throws {
        let failing = try DetailFixture.triple("postgresql").instance.testing(
            readiness: InstanceReadiness(
                state: .notReady, reason: "probe_failed", detail: "connection refused",
                observedAt: Date(timeIntervalSince1970: 0)))
        #expect(ReadinessText.text(for: failing) == "Not ready: connection refused")
        #expect(StatusAdapter.appearance(of: failing).label == "Not Ready")
        #expect(ConnectionCard.isNotAcceptingConnections(failing))
    }

    @Test(
        .evidence(
            covers: ["AC-012"],
            expected:
                "A connection that waited for its port becomes available after the detail reloads"
        ))
    func aConnectionThatWaitedForItsPortBecomesAvailableAfterStart() async throws {
        let stopped = try DetailFixture.triple("nats")
        let started = try DetailFixture.triple("postgresql")
        let script = DetailScript(detail: stopped.detail)
        let sleeper = ManualSleep()
        let model = InstanceDetailModel(
            instanceId: stopped.detail.instanceId, service: script.service,
            sleep: { try await sleeper.sleep(for: $0) })
        await model.open()
        model.observe(stopped.instance)
        #expect(
            model.detail?.connections.map(DetailFormat.unavailableText).allSatisfy { $0 != nil }
                == true)

        // Start: the instance moves to a new revision and the daemon now knows its port.
        let revision = stopped.instance.revision + 1
        let ready = InstanceDetail(
            instanceId: stopped.detail.instanceId, revision: revision,
            provider: started.detail.provider, runtime: started.detail.runtime,
            dataPath: started.detail.dataPath, startup: started.detail.startup,
            secrets: started.detail.secrets, dependents: [],
            connections: started.detail.connections)
        script.answer(detail: .success(ready))
        model.observe(stopped.instance.testing(revision: revision, lifecycle: .running))
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        try await waitUntil { model.detail?.revision == revision }

        #expect(model.detail?.connections.map(DetailFormat.unavailableText) == [nil, nil])
        #expect(DetailFormat.copyConnectionReason(model.detail) == nil)
    }

    @Test func noModelOfThePageHoldsASecretValueAfterAShowAndACopy() async throws {
        let credential = FixtureCredential()
        let item = try DetailFixture.triple("postgresql")
        let script = DetailScript(detail: item.detail, usage: item.dataUsage)
        let models = InstanceDetailModels(service: script.service)
        let model = models.model(for: item.detail.instanceId)
        await model.open()

        let secrets = SecretsScript { call in
            guard case .secretReveal(let params) = call else {
                return .failure(SecretsScript.rejection("secret.not_found"))
            }
            return .success(SecretsScript.revealReply(params.reference, credential.value))
        }
        let feedback = await ConnectionCopier.copy(
            try #require(item.detail.connections.first), secrets: secrets.client,
            pasteboard: ConcealedPasteboard(pasteboard: FakePasteboard()))

        var dumped = ""
        dump(models, to: &dumped)
        dump(model, to: &dumped)
        dump(feedback, to: &dumped)
        credential.assertAbsent(in: dumped, what: "the models and the feedback of the page")
    }
}
