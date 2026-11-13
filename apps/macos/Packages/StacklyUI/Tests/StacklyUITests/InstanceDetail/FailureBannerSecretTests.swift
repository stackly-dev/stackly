import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The failure banner never adds a secret of its own (spec 0033 AC-15): it shows fixed text,
/// the catalog details it knows (`endpoint`, `port`, `suggestedPort`, `otherInstance`), and
/// the excerpt the daemon already redacted. A fixture credential planted in every other
/// detail of the operation error never reaches the title, the message, the hint, the code
/// block of the Summary tab or the inspector, or a button, and a masked excerpt stays masked.
@MainActor
struct FailureBannerSecretTests {
    private let credential = FixtureCredential()

    /// The excerpt as the daemon sends it after `Redactor` saw it.
    private static let maskedExcerpt = [
        "auth failed for token ****",
        "connect postgresql://stackly:****@127.0.0.1:5432/app",
    ]

    private static let names: [UUID: String] = Dictionary(
        (FixtureScenario.instanceCases + FixtureFailureBanner.instances).map { ($0.id, $0.name) },
        uniquingKeysWith: { first, _ in first })

    /// Every text the banner can show for `content`, in the Summary tab and the inspector.
    private static func shownText(_ content: FailureBannerContent) -> String {
        var parts = [content.title, content.message]
        parts += [content.hint, content.summaryCode, content.inspectorCode].compactMap { $0 }
        parts += content.excerpt
        parts += (content.actions + content.inspectorActions).map(\.title)
        if content.portsChanged { parts.append("Its ports changed. Retry to start with them.") }
        return parts.joined(separator: "\n")
    }

    /// The details of `fixture`'s failed operation plus one detail per planted key, each
    /// holding the fixture credential in a form a client might pass along.
    private func planted(_ fixture: FixtureFailureBanner) -> (InstanceSummary, [OperationSummary]) {
        let secret = credential.value
        let extra = [
            ErrorDetail(
                key: "connectionString", value: "postgresql://stackly:\(secret)@127.0.0.1/app"),
            ErrorDetail(key: "authToken", value: secret),
            ErrorDetail(key: "env", value: "PASSWORD=\(secret)"),
        ]
        let operations = fixture.operations.map { $0.withPlantedDetails(extra) }
        let instance = fixture.instance.withExcerpt(Self.maskedExcerpt)
        return (instance, operations)
    }

    /// Whether a failed operation of `operations` carries the fixture credential, so a test
    /// knows the banner had it to leak.
    private static func carriesCredential(_ operations: [OperationSummary]) -> Bool {
        operations.contains { operation in
            (operation.failure?.error.details ?? []).contains {
                $0.value.contains(FixtureCredential.prefix)
            }
        }
    }

    private func resolve(_ fixture: FixtureFailureBanner) throws -> FailureBannerContent {
        let (instance, operations) = planted(fixture)
        if fixture != .restarting {
            #expect(Self.carriesCredential(operations), "the credential is planted")
        }
        return try #require(
            FailureBannerContent.resolve(
                instance, related: operations, instanceNames: { Self.names[$0] },
                now: FixtureClock.now))
    }

    // covers: spec 0033 AC-15 (nothing leaks from details the banner does not know)
    @Test(
        .evidence(
            covers: ["NFR-011", "VAL-020"],
            expected:
                "A crash loop banner whose operation error carries a fixture credential in details it does not know shows no part of the credential in its title, message, hint, code blocks, or buttons, and keeps the masked excerpt as the daemon sent it"
        ))
    func aCrashLoopBannerShowsNoCredentialFromDetailsItDoesNotKnow() throws {
        let content = try resolve(.crashLoop)

        #expect(content.rule == .crashLoop)
        #expect(content.operationId != nil, "the banner read the planted operation error")
        credential.assertAbsent(in: Self.shownText(content), what: "the crash loop banner")
        #expect(content.excerpt == Self.maskedExcerpt)
        #expect(content.summaryCode?.hasSuffix(Self.maskedExcerpt.joined(separator: "\n")) == true)
        #expect(content.inspectorCode == Self.maskedExcerpt.joined(separator: "\n"))
    }

    // covers: spec 0033 AC-15, AC-10 (only catalog details reach the port banner)
    @Test(
        .evidence(
            covers: ["NFR-011", "AC-010"],
            expected:
                "A port in use banner whose operation error also carries a fixture credential takes only the endpoint and the suggested port from its details: Use Port keeps the suggestion and no part of the credential reaches any text the banner shows"
        ))
    func aPortBannerTakesOnlyTheCatalogDetailsAndShowsNoCredential() throws {
        let content = try resolve(.portInUse)

        #expect(content.rule == .port)
        #expect(content.portFix == PortFix(endpoint: "database", port: 5439))
        #expect(content.message == "Port 5432 of database is held by another process.")
        credential.assertAbsent(in: Self.shownText(content), what: "the port in use banner")
    }

    // covers: spec 0033 AC-15, AC-12 (the restarting banner keeps the masked excerpt)
    @Test func aRestartingBannerShowsTheMaskedExcerptAndNoCredential() throws {
        let content = try resolve(.restarting)

        #expect(content.rule == .restarting)
        #expect(content.excerpt == Self.maskedExcerpt)
        credential.assertAbsent(in: Self.shownText(content), what: "the restarting banner")
    }
}

extension InstanceSummary {
    /// A copy whose last failure keeps its code, time, and exit, with `excerpt` instead.
    fileprivate func withExcerpt(_ excerpt: [String]) -> InstanceSummary {
        let failure = lastFailure.map {
            InstanceLastFailure(
                code: $0.code, at: $0.at, exitCode: $0.exitCode, signal: $0.signal,
                excerpt: excerpt)
        }
        return InstanceSummary(
            id: id, name: name, displayName: displayName, revision: revision,
            software: software, provider: provider, runtimeId: runtimeId,
            runtimeVersion: runtimeVersion, lifecycle: lifecycle, desired: desired,
            readiness: readiness, health: health, statusSeq: statusSeq, process: process,
            lastFailure: failure, restart: restart, restartTotal: restartTotal,
            blocker: blocker, endpoints: endpoints, leftoverProcesses: leftoverProcesses,
            logsUnavailable: logsUnavailable, ownership: ownership, initialized: initialized,
            capabilities: capabilities, lastExit: lastExit, autostart: autostart, environments: nil)
    }
}

extension OperationSummary {
    /// A copy whose failure error carries `extra` after its own details.
    fileprivate func withPlantedDetails(_ extra: [ErrorDetail]) -> OperationSummary {
        let planted = failure.map { info in
            OperationFailureInfo(
                reason: info.reason, retryable: info.retryable,
                error: ErrorBody(
                    code: info.error.code, category: info.error.category,
                    message: info.error.message, retryable: info.error.retryable,
                    resource: info.error.resource, fields: info.error.fields,
                    hint: info.error.hint, details: (info.error.details ?? []) + extra,
                    operationId: info.error.operationId, rule: info.error.rule,
                    items: info.error.items))
        }
        return OperationSummary(
            id: id, kind: kind, resource: resource, state: state, stage: stage,
            progress: progress, waitingOn: waitingOn, cancelRequestedAt: cancelRequestedAt,
            pointOfNoReturnAt: pointOfNoReturnAt, failure: planted, recovery: recovery,
            result: result, trigger: trigger, createdAt: createdAt, startedAt: startedAt,
            updatedAt: updatedAt, revision: revision, cancel: cancel, parent: nil)
    }
}
