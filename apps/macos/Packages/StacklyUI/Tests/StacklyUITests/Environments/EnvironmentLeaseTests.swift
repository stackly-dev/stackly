import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The lease in the app (spec 0047, AC-22, AC-23): the Lease row, "· lease" in the
/// inspector, and the override confirmation of a Stop an environment's lease refused.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct EnvironmentLeaseTests {
    private static let calendar = Calendar(identifier: .gregorian)
    private static let locale = Locale(identifier: "en_US")
    private static let timeZone = TimeZone(identifier: "UTC") ?? .gmt

    private static func lease(_ lease: EnvLeaseInfo?, now: Date = FixtureClock.now) -> String? {
        EnvironmentCopy.leaseText(
            lease, now: now, calendar: calendar, locale: locale, timeZone: timeZone)
    }

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "The Lease row says how many instances the lease holds and since when, says No lease without one, and hides for a daemon that sends none"
        ))
    func theLeaseRowFollowsTheDetail() throws {
        let now = try #require(
            Self.calendar.date(
                from: DateComponents(
                    timeZone: Self.timeZone, year: 2026, month: 11, day: 14, hour: 15)))
        let today = now.addingTimeInterval(-2 * 3600)
        #expect(
            Self.lease(EnvLeaseInfo(activeCount: 1, heldSince: today), now: now)
                == "Lease held on 1 instance since 1:00\u{202F}PM. Down stops only instances with no other lease or manual start."
        )
        let earlier = now.addingTimeInterval(-3 * 86_400)
        #expect(
            Self.lease(EnvLeaseInfo(activeCount: 3, heldSince: earlier), now: now)
                == "Lease held on 3 instances since Nov 11, 3:00\u{202F}PM. Down stops only instances with no other lease or manual start."
        )
        #expect(
            Self.lease(EnvLeaseInfo(activeCount: 0, heldSince: nil))
                == "No lease. Up takes one on each service’s instance.")
        #expect(Self.lease(nil) == nil)
    }

    @Test func theOwnershipRowSaysLeaseOnlyWhileALeaseIsActive() {
        let base = FixtureEnvironments.instances[0]
        #expect(InstanceInspector.ownershipText(of: base) == "Dedicated")
        let leased = base.with(environments: [
            InstanceEnvironment(
                environmentId: FixtureEnvironments.erpID, name: "erp-development",
                alias: "postgres", ownership: .dedicated, leaseActive: true)
        ])
        #expect(InstanceInspector.ownershipText(of: leased) == "Dedicated · lease")
    }

    // MARK: - Override

    private static func refusal(_ names: [String]) -> ErrorBody {
        ErrorBody(
            code: "supervisor.stop_requires_override", category: .conflict,
            message: "an environment holds this instance", retryable: false, resource: nil,
            fields: nil, hint: "stop it anyway with the override, or bring the environment down",
            details: nil, operationId: nil, rule: nil,
            items: names.map { name in
                ErrorItem(
                    kind: "environment",
                    fields: [
                        ErrorDetail(key: "environmentId", value: UUID().uuidString),
                        ErrorDetail(key: "name", value: name),
                        ErrorDetail(key: "alias", value: "db"),
                        ErrorDetail(key: "ownership", value: "shared"),
                    ])
            })
    }

    @Test func theOverrideConfirmationNamesTheEnvironments() {
        let one = InstanceActionModel.overrideRequest(
            name: "pg17-dev", body: Self.refusal(["reporting"]), perform: {})
        #expect(one.title == "Stop “pg17-dev”?")
        #expect(
            one.message
                == "It’s leased by reporting. Stopping it leaves the environment partially ready. Data is kept."
        )
        #expect(one.actionTitle == "Stop")
        #expect(one.role == .destructive)
        #expect(one.copyProblems.isEmpty)
        let two = InstanceActionModel.overrideRequest(
            name: "pg17-dev", body: Self.refusal(["erp-development", "reporting"]),
            perform: {})
        #expect(
            two.message
                == "It’s leased by erp-development, reporting. Stopping it leaves those environments partially ready. Data is kept."
        )
    }

    /// What the scripted daemon was sent.
    @MainActor
    final class Sent {
        var calls: [(Call, String)] = []
        var confirmations: [ConfirmationRequest] = []
        var feedback: [Feedback] = []
    }

    private func model(_ sent: Sent, outcomes: [MutationOutcome]) -> InstanceActionModel {
        var outcomes = outcomes
        return InstanceActionModel(
            send: { call, key in
                sent.calls.append((call, key))
                return outcomes.isEmpty
                    ? .rejected(Self.refusal(["reporting"])) : outcomes.removeFirst()
            },
            updates: { _ in AsyncStream { $0.finish() } },
            readOperation: { _ in nil })
    }

    @Test(
        .evidence(
            covers: ["REQ-028"],
            expected:
                "A Stop that supervisor.stop_requires_override refuses opens the destructive confirmation, and only its Stop sends instance.stop again with overrideLeases true and a new key"
        ))
    func aRefusedStopAsksAndOnlyTheConfirmationSendsTheOverride() async throws {
        let sent = Sent()
        let model = model(sent, outcomes: [.rejected(Self.refusal(["reporting"]))])
        let context = InstanceActionContext(
            report: { sent.feedback.append($0) },
            confirm: { sent.confirmations.append($0) })
        let summary = FixtureEnvironments.instances[0]
        await model.perform(.stop, for: summary, context: context)?.value
        #expect(sent.feedback.isEmpty)
        let request = try #require(sent.confirmations.first)
        #expect(request.message.contains("reporting"))
        guard case .instanceStop(let first) = try #require(sent.calls.first?.0) else {
            Issue.record("expected instance.stop")
            return
        }
        #expect(first.overrideLeases == false)

        request.perform()
        await Task.yield()
        for _ in 0..<20 where sent.calls.count < 2 { await Task.yield() }
        let second = try #require(sent.calls.last)
        guard case .instanceStop(let params) = second.0 else {
            Issue.record("expected instance.stop")
            return
        }
        #expect(params.overrideLeases)
        #expect(second.1 != sent.calls[0].1)
        // The override is refused again: it shows as feedback and asks no second time.
        for _ in 0..<20 where sent.feedback.isEmpty { await Task.yield() }
        #expect(sent.confirmations.count == 1)
        #expect(sent.feedback.map(\.code) == ["supervisor.stop_requires_override"])
    }

    @Test func withoutAPlaceToAskTheRefusalIsOnlyReported() async {
        let sent = Sent()
        let model = model(sent, outcomes: [])
        let context = InstanceActionContext(report: { sent.feedback.append($0) })
        await model.perform(.stop, for: FixtureEnvironments.instances[0], context: context)?
            .value
        #expect(sent.calls.count == 1)
        #expect(sent.feedback.map(\.code) == ["supervisor.stop_requires_override"])
    }
}
