import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The capacity model (spec 0022 AC-4): one call at a time, no old report over a newer one,
/// the poll, and the failures.
@Suite(.timeLimit(.minutes(1)))
struct CapacityModelTests {
    final class Script {
        var replies: [Result<Reply, ClientError>] = []
        var calls = 0
        var gate: CheckedContinuation<Void, Never>?
        var hold = false
        var sleeps: [Duration] = []
        var connected = true
        var stopAfterSleeps = 2

        func read(_ call: Call) async throws(ClientError) -> Reply {
            calls += 1
            if hold {
                hold = false
                await withCheckedContinuation { gate = $0 }
            }
            guard replies.isEmpty == false else { throw .notSent(Self.error("ipc.not_connected")) }
            return try replies.removeFirst().get()
        }

        static func error(_ code: String) -> ErrorBody {
            ErrorBody(
                code: code, category: .unavailable, message: "m", retryable: true, resource: nil,
                fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
        }
    }

    let script = Script()

    func makeModel() -> CapacityModel {
        let script = script
        return CapacityModel(
            read: { call throws(ClientError) in try await script.read(call) },
            sleep: { duration in
                script.sleeps.append(duration)
                if script.sleeps.count >= script.stopAfterSleeps { script.connected = false }
            })
    }

    func report(_ seconds: TimeInterval, level: CapacityLevel = .ok) -> Reply {
        .storageGetCapacity(
            InstanceContractFixture.capacity(
                level, checkedAt: Date(timeIntervalSince1970: seconds)))
    }

    @Test func nothingIsKnownBeforeTheFirstCall() {
        let model = makeModel()
        #expect(model.state == CapacityState())
        #expect(model.attempted == false)
    }

    @Test func aReplyFillsTheReportAndClearsEarlierFailures() async {
        let model = makeModel()
        script.replies = [
            .failure(.notSent(Script.error("ipc.not_connected"))), .success(report(10)),
        ]
        await model.refresh()
        #expect(model.report == nil)
        #expect(model.failure?.code == "ipc.not_connected")
        #expect(model.attempted)
        await model.refresh()
        #expect(model.report?.checkedAt == Date(timeIntervalSince1970: 10))
        #expect(model.failure == nil)
    }

    @Test func aReplyOlderThanTheReportHeldIsIgnored() async {
        let model = makeModel()
        script.replies = [.success(report(20, level: .low)), .success(report(10, level: .ok))]
        await model.refresh()
        await model.refresh()
        #expect(model.report?.level == .low)
        #expect(model.report?.checkedAt == Date(timeIntervalSince1970: 20))
    }

    @Test func aCallWhileOneRunsIsSkipped() async throws {
        let model = makeModel()
        script.hold = true
        script.replies = [.success(report(10))]
        let first = Task { await model.refresh() }
        while script.gate == nil { await Task.yield() }
        await model.refresh()
        #expect(script.calls == 1)
        script.gate?.resume()
        await first.value
        #expect(model.report != nil)
    }

    @Test func anOlderDaemonMakesTheMethodUnsupported() async {
        let model = makeModel()
        script.replies = [.failure(.rejected(Script.error("request.unknown_method")))]
        await model.refresh()
        #expect(model.unsupported)
        #expect(model.failure == nil)
        #expect(model.attempted)
    }

    @Test func aFailureAfterASuccessKeepsTheLastReport() async {
        let model = makeModel()
        script.replies = [
            .success(report(10, level: .critical)),
            .failure(.rejected(Script.error("storage.capacity_unavailable"))),
        ]
        await model.refresh()
        await model.refresh()
        #expect(model.report?.level == .critical)
        #expect(model.failure?.code == "storage.capacity_unavailable")
        #expect(model.state.report?.level == .critical)
    }

    @Test func thePollReadsEverySixtySecondsWhileConnected() async {
        let model = makeModel()
        script.replies = [.success(report(10)), .success(report(20)), .success(report(30))]
        let script = script
        await model.poll(isConnected: { script.connected })
        #expect(script.calls == 2)
        #expect(script.sleeps == [.seconds(60), .seconds(60)])
        #expect(model.report?.checkedAt == Date(timeIntervalSince1970: 20))
    }

    @Test func thePollDoesNothingWhileNotConnected() async {
        let model = makeModel()
        script.connected = false
        let script = script
        await model.poll(isConnected: { script.connected })
        #expect(script.calls == 0)
    }
}
