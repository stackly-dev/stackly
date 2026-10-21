import Foundation
import StacklyEvidenceCore
import Synchronization

/// The tests that called `evidence` and are still running, and what each one
/// noted so far.
///
/// XCTest calls the observer on its own queue, not on the main actor, and a
/// helper may be called from any thread, so the state sits behind a `Mutex`.
final class XCTEvidenceTracker: Sendable {
    /// One running test.
    struct Entry {
        var notes: XCTEvidenceMapping.Notes
        /// Where the records go, or nil when no run is active.
        var target: RunTarget?
    }

    private struct State {
        var entries: [ObjectIdentifier: Entry] = [:]
        /// The test the `XCTEvidence` functions talk to.
        var current: ObjectIdentifier?
    }

    private let state = Mutex(State())

    /// Starts tracking `key`. Returns false when it is already tracked, so a
    /// second `evidence` call in one test can be refused.
    func begin(_ key: ObjectIdentifier, record: Record, target: RunTarget?) -> Bool {
        state.withLock { state in
            guard state.entries[key] == nil else {
                return false
            }
            state.entries[key] = Entry(
                notes: XCTEvidenceMapping.Notes(record: record), target: target)
            state.current = key
            return true
        }
    }

    /// Whether `key` is tracked.
    func isTracking(_ key: ObjectIdentifier) -> Bool {
        state.withLock { $0.entries[key] != nil }
    }

    /// Notes the description of an issue; only the first one is kept.
    func noteIssue(_ key: ObjectIdentifier, description: String) {
        state.withLock { state in
            if state.entries[key]?.notes.firstIssue == nil {
                state.entries[key]?.notes.firstIssue = description
            }
        }
    }

    /// Notes the observed value of the current test. Returns false when no
    /// test that called `evidence` is running.
    func noteActual(_ value: String) -> Bool {
        state.withLock { state in
            guard let key = state.current, state.entries[key] != nil else {
                return false
            }
            state.entries[key]?.notes.actual = value
            return true
        }
    }

    /// Notes that the current test is blocked. Returns false when no test that
    /// called `evidence` is running.
    func noteBlocked(reason: String, owner: String) -> Bool {
        state.withLock { state in
            guard let key = state.current, state.entries[key] != nil else {
                return false
            }
            state.entries[key]?.notes.blocked = (reason, owner)
            return true
        }
    }

    /// Stops tracking `key` and returns what it noted.
    func finish(_ key: ObjectIdentifier) -> Entry? {
        state.withLock { state in
            if state.current == key {
                state.current = nil
            }
            return state.entries.removeValue(forKey: key)
        }
    }
}
