import Foundation
import StacklyClient
import Synchronization

@testable import StacklyUI

/// A scripted `instance.logs_read` and `instance.logs_follow`: records each call, answers
/// reads from a queue, and hands each follow a stream the test feeds and closes.
nonisolated final class LogsScript: Sendable {
    private struct State {
        var reads: [LogsReadParams] = []
        var follows: [LogsFollowParams] = []
        var unfollows = 0
        var readAnswers: [Result<LogPage, ClientError>] = []
        var lastRead: Result<LogPage, ClientError>
        var followAnswer: ClientError?
        var streams: [DaemonConnection.EventStream.Continuation] = []
        var head: LogCursor?
    }

    static let store = UUID(uuidString: "0195f2a0-0000-7000-8000-00000000a001") ?? UUID()
    static let retention = LogRetention(segmentsPerStream: 5, segmentMaxBytes: 20 << 20)

    private let state: Mutex<State>

    init(_ first: LogPage = LogsScript.page([])) {
        state = Mutex(State(lastRead: .success(first), head: first.head))
    }

    /// The reads so far, in order.
    var reads: [LogsReadParams] { state.withLock { $0.reads } }
    /// The follows so far, in order.
    var follows: [LogsFollowParams] { state.withLock { $0.follows } }
    /// How many follows were ended with unfollow.
    var unfollows: Int { state.withLock { $0.unfollows } }

    /// The next reads answer these, in order; after them the last one repeats.
    func answer(reads: Result<LogPage, ClientError>...) {
        state.withLock { state in
            state.readAnswers.append(contentsOf: reads)
            if let last = reads.last { state.lastRead = last }
        }
    }

    /// Every follow from now on fails with `error`, or succeeds with `nil`.
    func answer(follow error: ClientError?) {
        state.withLock { $0.followAnswer = error }
    }

    /// Sends a frame on the latest follow.
    func send(_ frame: LogFrame) {
        _ = state.withLock { $0.streams.last }?.yield(.log(frame))
    }

    /// Closes the connection of the latest follow: its stream ends with a throw.
    func dropConnection() {
        state.withLock { $0.streams.last }?.finish(
            throwing: ClientError.outcomeUnknown(ClientErrorCatalog.connectionClosed()))
    }

    var service: InstanceLogsService {
        InstanceLogsService(
            read: { [self] params throws(ClientError) in
                let answer = state.withLock { state in
                    state.reads.append(params)
                    return state.readAnswers.isEmpty
                        ? state.lastRead : state.readAnswers.removeFirst()
                }
                return try answer.get()
            },
            follow: { [self] params throws(ClientError) in
                let (stream, continuation) = DaemonConnection.EventStream.makeStream()
                let (failure, head) = state.withLock { state in
                    state.follows.append(params)
                    return (state.followAnswer, state.head)
                }
                if let failure { throw failure }
                state.withLock { $0.streams.append(continuation) }
                return LogFollowSession(
                    following: LogsFollowing(
                        subscriptionId: 1, storeId: Self.store, head: head),
                    frames: stream,
                    unfollow: { [self] in
                        state.withLock { $0.unfollows += 1 }
                        continuation.finish()
                    })
            })
    }

    // MARK: - Data

    static let start = Date(timeIntervalSince1970: 1_792_300_000)

    static func cursor(_ seq: UInt64) -> LogCursor {
        LogCursor(storeId: store, seq: seq)
    }

    static func line(
        _ seq: UInt64, _ text: String = "line", stream: LogStream = .stdout,
        level: StacklyClient.LogLevel = .normal, at: Date? = nil
    ) -> LogEntry {
        LogEntry(
            seq: seq, at: at ?? start.addingTimeInterval(Double(seq) / 1_000), stream: stream,
            kind: .line(
                LogLineEntry(text: text, level: level, partial: nil, continued: nil, step: nil)))
    }

    static func lines(_ range: ClosedRange<UInt64>) -> [LogEntry] {
        range.map { line($0) }
    }

    /// A page; `head` is the last entry when left out.
    static func page(
        _ entries: [LogEntry], earlier: LogCursor? = nil, later: LogCursor? = nil,
        head: LogCursor? = nil, gap: LogGap? = nil, scanLimited: Bool = false
    ) -> LogPage {
        LogPage(
            storeId: store, entries: entries, earlier: earlier, later: later,
            head: head ?? entries.last.map { cursor($0.seq) }, gap: gap,
            scanLimited: scanLimited, retention: retention,
            storedBytes: LogStreamBytes(stdout: 0, stderr: 0), skippedRecords: 0)
    }

    static func frame(_ entries: [LogEntry], gap: LogGap? = nil, ended: Bool = false) -> LogFrame {
        LogFrame(
            subscriptionId: 1, storeId: store, entries: entries, gap: gap,
            ended: ended ? LogFollowEnd(reason: .instanceRemoved) : nil)
    }
}
