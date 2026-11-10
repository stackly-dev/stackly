import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

/// The Swift side of the spec 0035 contract (AC-17): the `audit.list` page and the
/// `cancel` of an operation summary decode from the shared fixtures, and every new open enum
/// takes a value from a newer daemon as `unrecognized`.
struct AuditContractTests {
    private static func page(_ file: String) throws -> AuditPage {
        guard case .auditList(let page) = try CreateContractTests.reply(file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return page
    }

    @Test(
        .evidence(
            covers: ["REQ-045"],
            expected:
                "every row of the audit.list fixture decodes with its actor, client, operation, instance, revision, result, and details in key order"
        ))
    func theAuditPageDecodesEveryRow() throws {
        let page = try Self.page("frame-response-audit-list.json")
        let items = try #require(page.items)
        #expect(items.count == 5)
        #expect(page.next == "2026-11-09T21:00:00.000000000Z_0192f3a6-0000-7000-8000-0000000000c5")

        let cancel = items[0]
        #expect(cancel.action == "operation.cancel")
        #expect(cancel.actor == .user)
        #expect(cancel.client == .gui)
        #expect(cancel.operationKind == .installRuntime)
        #expect(cancel.resource?.kind == .runtime)
        #expect(cancel.instanceId == nil)
        #expect(
            cancel.details == [
                AuditDetail(key: "disposition", value: "continuing_past_point_of_no_return")
            ])

        let start = items[1]
        #expect(start.action == "operation.finished")
        #expect(start.client == .cli)
        #expect(start.operationKind == .startInstance)
        #expect(start.result == .failed)
        #expect(start.configRevision == 3)
        #expect(start.instanceId != nil)
        let keys = try #require(start.details).map(\.key)
        #expect(keys == keys.sorted())

        #expect(items[2].action == "secret.reveal")
        #expect(items[2].operationKind == nil)
        #expect(items[3].result == .needsRecovery)
        #expect(items[3].client == nil)
        #expect(items[4].result == .cancelled)
        // Rows come newest first.
        #expect(zip(items, items.dropFirst()).allSatisfy { $0.occurredAt >= $1.occurredAt })
    }

    @Test func valuesFromANewerDaemonDecodeAsUnrecognized() throws {
        let page = try Self.page("frame-response-audit-list-unknown-values.json")
        let row = try #require(page.items?.first)
        #expect(row.actor == .unrecognized)
        #expect(row.client == .unrecognized)
        #expect(row.operationKind == .unrecognized)
        #expect(row.result == .unrecognized)
        // An action is open text: it stays as the daemon wrote it.
        #expect(row.action == "operation.paused")
        #expect(page.next == nil)
    }

    @Test func eachOperationSummaryCarriesItsCancelAvailability() throws {
        let file = "frame-response-operation-list-cancel.json"
        guard case .operationList(let page) = try CreateContractTests.reply(file) else {
            throw FixtureShapeError.unexpected(file)
        }
        let cancels = try #require(page.items).map(\.cancel)
        #expect(cancels == [.pastPointOfNoReturn, .continuing, .requested, .available])
    }
}
