import Foundation
import StacklyClient
import Testing

/// The `storage` area of protocol 1.9 and the instance test data from the
/// shared fixtures (spec 0022 AC-1 and AC-12): the capacity reply and its
/// errors decode, a foreign level reads as `.unrecognized`, the timeout
/// error carries its static hint, and `groups.json`, `list-mixed.json`, and
/// `attention-operations.json` decode with the generated types.
struct StorageContractTests {
    static func report(_ file: String) throws -> CapacityReport {
        guard case .response(let response) = try Fixture.ipc(ServerFrame.self, file),
            case .storageGetCapacity(let report) = response.reply
        else {
            throw FixtureShapeError.unexpected(file)
        }
        return report
    }

    static func error(_ file: String) throws -> ErrorBody {
        guard case .error(let frame) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return frame.error
    }

    static func instance<Value: Decodable>(_ type: Value.Type, _ file: String) throws -> Value {
        try StacklyJSON.decoder().decode(type, from: Fixture.data("instance/\(file)"))
    }

    @Test func theCapacityRequestIsAUnitMethod() throws {
        let request = try Fixture.ipc(Request.self, "request-storage-get-capacity.json")
        #expect(request.call == .storageGetCapacity)
        #expect(request.call.methodName == "storage.get_capacity")
        #expect(request.idempotencyKey == nil)
    }

    @Test func aReportCarriesTheSizesThePercentAndTheLevel() throws {
        let ok = try Self.report("frame-response-storage-get-capacity.json")
        #expect(ok.level == .ok)
        #expect(ok.usedPercent == 58)
        #expect(ok.totalBytes == 994_662_584_320)
        #expect(ok.availableBytes == 412_317_048_832)
        let critical = try Self.report("frame-response-storage-get-capacity-critical.json")
        #expect(critical.level == .critical)
        #expect(critical.availableBytes < 2_000_000_000)
        #expect(critical.checkedAt > ok.checkedAt)
    }

    @Test func aForeignLevelReadsAsUnrecognized() throws {
        let report = try Self.report("frame-response-storage-get-capacity-unknown-level.json")
        #expect(report.level == .unrecognized)
    }

    @Test func theUnavailableErrorsAreRetryableAndOnlyATimeoutHasAHint() throws {
        let timeout = try Self.error("frame-error-storage-capacity-unavailable-timeout.json")
        let io = try Self.error("frame-error-storage-capacity-unavailable-io.json")
        let noSize = try Self.error("frame-error-storage-capacity-unavailable-no-size.json")
        for error in [timeout, io, noSize] {
            #expect(error.code == "storage.capacity_unavailable")
            #expect(error.category == .unavailable)
            #expect(error.retryable)
        }
        #expect(timeout.details?.first { $0.key == "reason" }?.value == "timeout")
        #expect(
            timeout.hint
                == "The disk that holds Stackly data didn’t answer. Check that it is still connected."
        )
        #expect(io.details?.map(\.key) == ["reason", "kind"])
        #expect(io.hint == nil)
        #expect(noSize.hint == nil)
    }

    private struct GroupCases: Decodable {
        struct Case: Decodable {
            let lifecycle: InstanceLifecycle
            let readiness: InstanceReadinessState
            let group: String?
        }
        let cases: [Case]
    }

    @Test func groupsJSONCoversEveryLifecycleTimesEveryReadinessState() throws {
        let cases = try Self.instance(GroupCases.self, "groups.json").cases
        #expect(cases.count == 30)
        #expect(Set(cases.map { "\($0.lifecycle)/\($0.readiness)" }).count == 30)
        for entry in cases where entry.lifecycle == .retained {
            #expect(entry.group == nil)
        }
        let running = cases.filter { $0.lifecycle == .running }
        #expect(running.filter { $0.group == "ready" }.map(\.readiness) == [.ready])
        #expect(running.filter { $0.group == "notReady" }.count == 2)
    }

    @Test func theMixedListDecodesWithTwentyDistinctInstances() throws {
        let page = try Self.instance(InstancePage.self, "list-mixed.json")
        let items = try #require(page.items)
        #expect(items.count == 20)
        #expect(page.next == nil)
        #expect(Set(items.map(\.id)).count == 20)
        #expect(Set(items.map(\.name)).count == 20)
        #expect(items.filter { $0.lifecycle == .failed }.count == 3)
        #expect(items.first { $0.name == "half-made" }?.initialized == false)
        #expect(items.allSatisfy { $0.ownership == .manual })
    }

    @Test func theAttentionOperationsHoldEveryRecoveryHint() throws {
        let page = try Self.instance(OperationPage.self, "attention-operations.json")
        let items = try #require(page.items)
        let hints = items.compactMap { $0.recovery?.hint }
        #expect(hints == [.resume, .abandon, .manual, .autoCompensate, .unrecognized])
        #expect(items.first { $0.state == .running }?.stage == "downloading")
        #expect(items.contains { $0.state == .cancelRequested })
    }
}
