import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// The calls the live services of the app put on the wire (spec 0035 AC-11, AC-15, and the
/// banner of spec 0033): the history of the Operations page, the newest operations and the
/// Activity page of an instance, each read through the daemon model's link. The models only
/// see the closures, so these tests pin the params a closure sends, and that a reply of
/// another method is a protocol violation, never a page.
@MainActor
struct LiveServiceCallsTests {
    private static let instance =
        UUID(uuidString: "0192f3a6-0000-7000-8000-0000000001aa") ?? UUID()

    /// A daemon model whose link answers every call with `reply` and records what it sent.
    private func rig(_ reply: Reply) -> (DaemonModel, DaemonModelTests.FakeLink) {
        let link = DaemonModelTests.FakeLink()
        link.result.withLock { $0 = .success(reply) }
        let model = DaemonModel(
            link: link.handle, signals: nil, endpoint: nil, launchedAt: DaemonModelTests.launch)
        return (model, link)
    }

    private func sent(_ link: DaemonModelTests.FakeLink) -> [Call] {
        link.calls.withLock { $0.map(\.0) }
    }

    private func isProtocolViolation(_ error: ClientError) -> Bool {
        guard case .rejected(let body) = error else { return false }
        return body.code == ClientErrorCatalog.protocolViolation().code
    }

    // MARK: - Instance detail (spec 0033 banner, spec 0035 AC-15)

    @Test func theNewestOperationsOfAnInstanceAskForThatInstanceWithTheLimit() async throws {
        let summary = OperationSample.make(instance: Self.instance, state: .failed)
        let (model, link) = rig(.operationList(OperationPage(items: [summary], next: nil)))
        let service = InstanceDetailService.live(model)

        let items = try await service.operations(Self.instance, 5)

        #expect(items == [summary])
        #expect(
            sent(link) == [
                .operationList(
                    ListOperationsParams(
                        activeOnly: nil,
                        resource: ResourceRef(
                            kind: .instance, id: Self.instance.uuidString.lowercased()),
                        limit: 5, before: nil))
            ])
        #expect(link.calls.withLock { $0.map(\.1) } == [nil], "a read sends no idempotency key")
    }

    @Test func aPageWithoutItemsIsNoOperations() async throws {
        let (model, _) = rig(.operationList(OperationPage(items: nil, next: nil)))
        let items = try await InstanceDetailService.live(model).operations(Self.instance, 5)
        #expect(items.isEmpty)
    }

    @Test func anotherReplyToTheOperationsOfAnInstanceIsAProtocolViolation() async {
        let (model, _) = rig(.auditList(AuditPage(items: [], next: nil)))
        do {
            _ = try await InstanceDetailService.live(model).operations(Self.instance, 5)
            Issue.record("the read succeeded")
        } catch {
            #expect(isProtocolViolation(error), "\(error)")
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-045"],
            expected:
                "the Activity tab of the app reads audit.list with the instance as instanceId, no operationId, a limit of 50, and the cursor of Show More"
        ))
    func theActivityOfAnInstanceAsksAuditListForThatInstanceWithItsCursor() async throws {
        let row = ActivitySample.row()
        let (model, link) = rig(.auditList(AuditPage(items: [row], next: "cursor-2")))
        let service = InstanceDetailService.live(model)

        let first = try await service.activity(Self.instance, nil)
        let more = try await service.activity(Self.instance, "cursor-1")

        #expect(first.items == [row])
        #expect(first.next == "cursor-2")
        #expect(more.items == [row])
        #expect(
            sent(link) == [
                .auditList(
                    ListAuditParams(
                        instanceId: Self.instance, operationId: nil,
                        limit: InstanceActivityModel.pageSize, before: nil)),
                .auditList(
                    ListAuditParams(
                        instanceId: Self.instance, operationId: nil,
                        limit: InstanceActivityModel.pageSize, before: "cursor-1")),
            ])
        #expect(InstanceActivityModel.pageSize == 50)
    }

    @Test func anotherReplyToTheActivityIsAProtocolViolation() async {
        let (model, _) = rig(.operationList(OperationPage(items: [], next: nil)))
        do {
            _ = try await InstanceDetailService.live(model).activity(Self.instance, nil)
            Issue.record("the read succeeded")
        } catch {
            #expect(isProtocolViolation(error), "\(error)")
        }
    }

    // MARK: - Operations page (spec 0035 AC-11, AC-12)

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "the history of the Operations page reads operation.list without activeOnly or a resource, 50 per page, with the cursor of Show More"
        ))
    func theHistoryAsksForEveryOperationFiftyAtATimeBeforeTheCursor() async throws {
        let summary = OperationSample.make()
        let (model, link) = rig(.operationList(OperationPage(items: [summary], next: "c2")))
        let service = OperationsService.live(model)

        let first = try await service.list(nil)
        _ = try await service.list("c1")

        #expect(first.items == [summary])
        #expect(first.next == "c2")
        #expect(
            sent(link) == [
                .operationList(
                    ListOperationsParams(
                        activeOnly: false, resource: nil, limit: 50, before: nil)),
                .operationList(
                    ListOperationsParams(
                        activeOnly: false, resource: nil, limit: 50, before: "c1")),
            ])
        #expect(OperationsService.pageSize == 50)
    }

    @Test func theInspectorReadsTheOperationItAsksForAndRefusesAnother() async throws {
        let shown = OperationSample.make(1)
        let (model, link) = rig(.operationGet(OperationsFake.detail(shown)))
        let service = OperationsService.live(model)

        let detail = try await service.get(shown.id)
        #expect(detail.operation == shown)
        #expect(sent(link) == [.operationGet(GetOperationParams(operationId: shown.id))])

        // A reply about another operation is not the one the inspector asked for.
        do {
            _ = try await service.get(OperationSample.id(2))
            Issue.record("the read succeeded")
        } catch {
            #expect(isProtocolViolation(error), "\(error)")
        }
    }

    @Test func anotherReplyToTheHistoryIsAProtocolViolation() async {
        let (model, _) = rig(.auditList(AuditPage(items: [], next: nil)))
        do {
            _ = try await OperationsService.live(model).list(nil)
            Issue.record("the read succeeded")
        } catch {
            #expect(isProtocolViolation(error), "\(error)")
        }
    }
}
