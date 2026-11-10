import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization
import Testing

/// `audit.list` of `StacklyClient` over a fake `send` (spec 0035 AC-5, AC-17): the client
/// sends the params of the contract, reads the matching reply, and treats a reply of another
/// kind as a protocol violation.
struct AuditClientTests {
    /// Records each call and answers with a fixed result.
    final class Fake: Sendable {
        private let calls = Mutex<[Call]>([])
        private let answer: Mutex<Result<Reply, ClientError>>

        init(_ answer: Result<Reply, ClientError>) {
            self.answer = Mutex(answer)
        }

        var made: [Call] { calls.withLock { $0 } }

        var client: AuditClient {
            AuditClient { [self] call, _ throws(ClientError) in
                calls.withLock { $0.append(call) }
                return try answer.withLock { $0 }.get()
            }
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-045"],
            expected:
                "auditList sends the filters, limit, and cursor of the fixture request and returns the page of the fixture reply"
        ))
    func auditListSendsTheFiltersAndReadsThePage() async throws {
        let request = try Fixture.ipc(Request.self, "request-audit-list.json")
        guard case .auditList(let params) = request.call else {
            Issue.record("the fixture is not an audit.list request")
            return
        }
        let reply = try CreateContractTests.reply("frame-response-audit-list.json")
        guard case .auditList(let expected) = reply else {
            Issue.record("the fixture is not an audit.list reply")
            return
        }
        let fake = Fake(.success(reply))

        let page = try await fake.client.auditList(
            instanceId: params.instanceId, operationId: params.operationId, limit: params.limit,
            before: params.before)

        #expect(fake.made == [request.call])
        #expect(page == expected)
    }

    @Test func auditListWithoutArgumentsSendsNoFilter() async throws {
        let request = try Fixture.ipc(Request.self, "request-audit-list-without-fields.json")
        let fake = Fake(.success(.auditList(AuditPage(items: [], next: nil))))

        let page = try await fake.client.auditList()

        #expect(fake.made == [request.call])
        #expect(page.items?.isEmpty == true)
        #expect(page.next == nil)
    }

    @Test func auditListRefusesAReplyOfAnotherKind() async throws {
        let fake = Fake(
            .success(try CreateContractTests.reply("frame-response-operation-list-cancel.json")))
        do {
            _ = try await fake.client.auditList()
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .rejected(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func auditListSendsALimitTheDaemonRefusesAsItIsAndPassesTheErrorOn() async throws {
        // The daemon, not the client, judges the limit (`reject-audit-list-limit.json`).
        let request = try Fixture.ipc(Request.self, "reject-audit-list-limit.json")
        let failure = ClientError.notSent(ClientErrorCatalog.notConnected())
        let fake = Fake(.failure(failure))
        do {
            _ = try await fake.client.auditList(limit: AuditClient.maxLimit + 1)
            Issue.record("expected the error")
        } catch {
            #expect(error == failure)
        }
        #expect(fake.made == [request.call])
    }
}
