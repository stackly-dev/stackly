import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The doctor model (spec 0034, AC-20): when it runs, one call at a time, the report that
/// stays, the failures, and the capacity it hands on. The daemon is a script.
@Suite(.timeLimit(.minutes(1)))
struct DoctorModelTests {
    final class Script {
        var replies: [Result<Reply, ClientError>] = []
        var calls: [Call] = []
        var gate: CheckedContinuation<Void, Never>?
        var hold = false

        func read(_ call: Call) async throws(ClientError) -> Reply {
            calls.append(call)
            if hold {
                hold = false
                await withCheckedContinuation { gate = $0 }
            }
            guard replies.isEmpty == false else { throw .notSent(Self.error("ipc.not_connected")) }
            return try replies.removeFirst().get()
        }

        func release() {
            gate?.resume()
            gate = nil
        }

        static func error(_ code: String) -> ErrorBody {
            ErrorBody(
                code: code, category: .unavailable, message: "m", retryable: true, resource: nil,
                fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
        }
    }

    let script = Script()
    let start = Date(timeIntervalSince1970: 1_800_000_000)

    func makeModel(
        capacity: CapacityModel? = nil, now: @escaping () -> Date = { Date() }
    ) -> DoctorModel {
        let script = script
        return DoctorModel(
            read: { call throws(ClientError) in try await script.read(call) }, capacity: capacity,
            now: now)
    }

    func clean(_ seconds: TimeInterval = 0, checkedAt: Date? = nil) throws -> Reply {
        .doctorRun(
            try DoctorFixture.clean(
                finishedAt: start.addingTimeInterval(seconds), checkedAt: checkedAt))
    }

    /// Waits until the script holds a call.
    func waitForHeldCall() async {
        while script.gate == nil { await Task.yield() }
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "A run sends doctor.run for everything, with no instance and no check, and keeps the report"
        ))
    func aRunChecksEverything() async throws {
        let model = makeModel()
        script.replies = [.success(.doctorRun(try DoctorFixture.mixed()))]
        await model.request(.user)
        guard case .doctorRun(let params) = try #require(script.calls.first) else {
            Issue.record("not a doctor.run call")
            return
        }
        #expect(params.instanceId == nil)
        #expect(params.checks == nil)
        #expect(model.report?.errors == 3)
        #expect(model.errorCount == 3)
        #expect(model.lastRunAt == model.report?.finishedAt)
        #expect(model.running == false)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "A caught up shell runs the doctor once per server instance; the same instance again runs nothing"
        ))
    func caughtUpRunsOncePerServerInstance() async throws {
        let model = makeModel()
        let first = UUID()
        script.replies = [.success(try clean()), .success(try clean(1))]
        await model.request(.caughtUp(serverInstanceId: first))
        await model.request(.caughtUp(serverInstanceId: first))
        #expect(script.calls.count == 1)
        await model.request(.caughtUp(serverInstanceId: UUID()))
        #expect(script.calls.count == 2)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "Diagnostics appearing runs the doctor without a report or with one older than 5 minutes, and not with a fresh one"
        ))
    func appearingRunsOnlyWithoutAFreshReport() async throws {
        var clock = start
        let model = makeModel(now: { clock })
        script.replies = [.success(try clean()), .success(try clean(400))]
        await model.request(.appeared)
        #expect(script.calls.count == 1)
        clock = start.addingTimeInterval(300)
        await model.request(.appeared)
        #expect(script.calls.count == 1)
        clock = start.addingTimeInterval(301)
        await model.request(.appeared)
        #expect(script.calls.count == 2)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "A trigger during a run is ignored, except Run Doctor, which starts one more run after it"
        ))
    func onlyRunDoctorQueuesDuringARun() async throws {
        let model = makeModel()
        script.replies = [.success(try clean()), .success(try clean(5))]
        script.hold = true
        let first = Task { await model.request(.opened) }
        await waitForHeldCall()
        #expect(model.running)
        await model.request(.opened)
        await model.request(.appeared)
        await model.request(.caughtUp(serverInstanceId: UUID()))
        #expect(script.calls.count == 1)
        await model.request(.user)
        await model.request(.user)
        script.release()
        await first.value
        #expect(script.calls.count == 2)
        #expect(model.report?.finishedAt == start.addingTimeInterval(5))
        #expect(model.running == false)
    }

    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected:
                "A failed run keeps the last report and records the error; the next success clears it"
        ))
    func aFailureKeepsTheReport() async throws {
        let model = makeModel()
        script.replies = [
            .success(try clean()), .failure(.outcomeUnknown(Script.error("ipc.timeout"))),
            .success(try clean(9)),
        ]
        await model.request(.user)
        await model.request(.user)
        #expect(model.report?.finishedAt == start)
        #expect(model.failure?.code == "ipc.timeout")
        await model.request(.user)
        #expect(model.failure == nil)
        #expect(model.report?.finishedAt == start.addingTimeInterval(9))
    }

    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected:
                "A daemon that does not know doctor.run makes the model unsupported until a new server instance"
        ))
    func anUnknownMethodIsUnsupportedUntilANewDaemon() async throws {
        let model = makeModel()
        script.replies = [
            .failure(.rejected(Script.error("request.unknown_method"))), .success(try clean()),
        ]
        await model.request(.caughtUp(serverInstanceId: UUID()))
        #expect(model.unsupported)
        #expect(model.failure == nil)
        await model.request(.user)
        #expect(script.calls.count == 1)
        await model.request(.caughtUp(serverInstanceId: UUID()))
        #expect(model.unsupported == false)
        #expect(model.report != nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "The capacity of the disk_space check reaches the capacity model, and an older one never replaces a newer report"
        ))
    func theDiskCapacityReachesTheCapacityModel() async throws {
        let capacity = CapacityModel(
            read: { _ throws(ClientError) in throw .notSent(Script.error("ipc.not_connected")) },
            sleep: { _ in })
        let model = makeModel(capacity: capacity)
        let newer = start.addingTimeInterval(60)
        script.replies = [
            .success(try clean(checkedAt: newer)), .success(try clean(1, checkedAt: start)),
        ]
        await model.request(.user)
        #expect(capacity.report?.checkedAt == newer)
        #expect(capacity.attempted)
        await model.request(.user)
        #expect(capacity.report?.checkedAt == newer)
    }
}
