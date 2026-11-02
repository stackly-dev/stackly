import Foundation
import StacklyClient
import Synchronization

@testable import StacklyUI

/// A scripted `instance.get_metrics`: records each call with its cursor and answers from a
/// queue, repeating the last answer when the queue runs out.
nonisolated final class MetricsScript: Sendable {
    /// One call as the model made it.
    struct Call: Equatable, Sendable {
        let instance: UUID
        let historyId: String?
        let afterSeq: UInt64?
    }

    private struct State {
        var calls: [Call] = []
        var answers: [Result<MetricsReply, ClientError>] = []
        var last: Result<MetricsReply, ClientError>
    }

    static let instance = UUID(uuidString: "0195f2a0-0000-7000-8000-0000000000b1") ?? UUID()
    static let history = "4242-1760950802.000000"

    private let state: Mutex<State>

    init(_ first: MetricsReply = MetricsScript.reply(samples: [])) {
        state = Mutex(State(last: .success(first)))
    }

    /// The calls so far, in order.
    var calls: [Call] { state.withLock { $0.calls } }

    /// The next calls answer these, in order; after them the last one repeats.
    func answer(_ answers: Result<MetricsReply, ClientError>...) {
        state.withLock { state in
            state.answers.append(contentsOf: answers)
            if let last = answers.last { state.last = last }
        }
    }

    var service: InstanceMetricsService {
        InstanceMetricsService { [self] id, historyId, afterSeq throws(ClientError) in
            let answer = state.withLock { state in
                state.calls.append(Call(instance: id, historyId: historyId, afterSeq: afterSeq))
                return state.answers.isEmpty ? state.last : state.answers.removeFirst()
            }
            return try answer.get()
        }
    }

    // MARK: - Data

    /// A sample with a CPU of `cpu` percent for the main process and ten times that for the
    /// tree.
    static func sample(_ seq: UInt64, cpu: Double = 1) -> MetricsSample {
        MetricsSample(
            seq: seq, sampledAt: FixtureClock.secondsAgo(Int(100 - seq)),
            main: ResourceUsage(
                scope: .main, processCount: 1, measuredCount: 1, cpu: .value(cpu),
                memory: .value(1_000_000), diskRead: .value(0), diskWrite: .value(0)),
            tree: ResourceUsage(
                scope: .tree, processCount: 3, measuredCount: 3, cpu: .value(cpu * 10),
                memory: .value(3_000_000), diskRead: .value(0), diskWrite: .value(0)))
    }

    static func samples(_ range: ClosedRange<UInt64>) -> [MetricsSample] {
        range.map { sample($0) }
    }

    static func row(_ pid: UInt32, depth: UInt32 = 0, name: String = "postgres") -> ProcessRow {
        ProcessRow(
            pid: pid, parentPid: depth == 0 ? nil : 4242, depth: depth, name: name,
            isMain: depth == 0, cpu: .value(0.5), memory: .value(1_000_000))
    }

    /// A live reply; pass what differs.
    static func reply(
        samples: [MetricsSample], reset: Bool = true, historyId: String? = history,
        processes: [ProcessRow]? = nil, truncated: Bool = false, capacity: UInt64 = 60,
        interval: UInt32 = 5, state: MetricsState = .live
    ) -> MetricsReply {
        MetricsReply(
            instanceId: instance, state: state, lifecycle: state == .live ? .running : .stopped,
            intervalSeconds: interval, capacity: capacity,
            historyId: state == .live ? historyId : nil,
            process: state == .live
                ? MetricsProcess(pid: 4242, startedAt: FixtureClock.minutesAgo(30)) : nil,
            samples: samples, latestProcesses: processes, processesTruncated: truncated,
            historyReset: reset, restartTotal: 0, lastFailure: nil,
            readiness: .fixture(.ready, reason: "probe_ok"),
            health: .fixture(.healthy, reason: "probe_ok"),
            dataUsage: .unavailable(UnavailableContent(reason: .notScanned)))
    }

    static let refusal = ErrorBody(
        code: "instance.not_found", category: .notFound, message: "no such instance",
        retryable: false, resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
        rule: nil, items: nil)
}
