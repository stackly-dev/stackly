import Foundation
import Observation
import StacklyClient

/// The instances the daemon manages, kept from snapshots plus `instance.` events (spec 0013,
/// *Daftar instance sementara*).
///
/// Its store loads `instance.list` and follows `instance.state_changed`,
/// `instance.health_changed`, and `instance.removed`. A retained instance never enters it,
/// by any path, so the list, the badge, and the count never include one.
///
/// Two clocks order the changes (spec 0010): the row's `revision` orders whole summaries, and
/// `statusSeq` orders the status fields (`readiness`, `health`, `blocker`, `restart`,
/// `process`, `logsUnavailable`). `instance.health_changed` applies by `statusSeq` alone and
/// never moves the held revision, so an `instance.state_changed` at that revision still
/// applies. The `statusSeq` bound resets when the daemon restarts, because the counter
/// starts again at 1 in every run.
@Observable
public final class InstanceStore {
    /// The most instances one `instance.list` page holds.
    static var pageLimit: UInt32 { 200 }

    /// The store the shell keeps in sync.
    public let store: ResourceStore<InstanceSummary>

    /// Creates an empty store.
    ///
    /// - Parameters:
    ///   - fetchPage: Fetches one page of instances.
    ///   - fetchOne: Fetches one instance, or `nil` when it is gone or retained.
    ///   - now: The wall clock.
    ///   - sleep: Waits; tests inject one that does not.
    public init(
        fetchPage: @escaping (PageToken?) async throws(ClientError) -> Page<InstanceSummary>,
        fetchOne: @escaping (UUID) async throws(ClientError) -> InstanceSummary?,
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        store = ResourceStore(
            kinds: ["instance."],
            fetchPage: fetchPage,
            fetchOne: fetchOne,
            revision: { $0.revision },
            reduce: Self.reduce,
            merge: Self.merge,
            isGone: { $0.lifecycle == .retained },
            resetObservations: { $0 = $0.with(statusSeq: 0) },
            now: now,
            sleep: sleep)
    }

    /// A store that reads through `call`: `instance.list` for snapshots and `instance.get`
    /// for one instance.
    ///
    /// - Parameters:
    ///   - call: Sends one call to the daemon.
    ///   - now: The wall clock.
    public convenience init(
        call: @escaping (Call) async throws(ClientError) -> Reply,
        now: @escaping () -> Date = { Date() }
    ) {
        self.init(
            fetchPage: { token throws(ClientError) in try await Self.page(after: token, call: call)
            },
            fetchOne: { id throws(ClientError) in try await Self.instance(id, call: call) },
            now: now)
    }

    // MARK: - What the shell shows

    /// The number of instances: none of them retained.
    public var count: Int {
        store.count
    }

    /// The number the sidebar badge shows, or `nil` before the first snapshot loaded, so
    /// no badge shows a number the daemon never sent.
    public var loadedCount: Int? {
        store.loadedAt == nil ? nil : store.count
    }

    /// The instances by name, then ID.
    public var instances: [InstanceSummary] {
        store.items.values.sorted(by: Self.byName)
    }

    /// The Instances page subtitle ("1 instance", "3 instances"), or `nil` before the first
    /// snapshot loaded.
    public var subtitle: String? {
        Self.subtitle(count: loadedCount)
    }

    /// The Instances page subtitle for a count, or `nil` when the count never loaded.
    public static func subtitle(count: Int?) -> String? {
        count.map { SidebarSection.countText($0, singular: "instance", plural: "instances") }
    }

    /// Applies an instance fetched outside a snapshot, under the same rules as an event. A
    /// retained instance is dropped.
    func offer(_ instance: InstanceSummary) {
        store.offer(instance)
    }

    // MARK: - Rules

    /// By name with the Finder's rules, then by ID.
    static func byName(_ a: InstanceSummary, _ b: InstanceSummary) -> Bool {
        switch a.name.localizedStandardCompare(b.name) {
        case .orderedAscending: true
        case .orderedDescending: false
        case .orderedSame: a.id.uuidString < b.id.uuidString
        }
    }

    /// What an `instance.` event does to the store.
    static func reduce(_ event: Event) -> StoreEffect<InstanceSummary> {
        switch event.kind {
        case .instanceStateChanged(let changed):
            let summary = changed.instance
            if summary.lifecycle == .retained {
                return .delete(summary.id, revision: event.revision ?? summary.revision)
            }
            return .upsert(summary)
        case .instanceHealthChanged(let change):
            guard let revision = event.revision else { return .ignore }
            return .observe(change.instanceId, revision: revision) { held in
                guard change.statusSeq > held.statusSeq else { return false }
                held = held.applying(change)
                return true
            }
        case .instanceRemoved(let removed):
            guard let revision = event.revision else { return .ignore }
            return .delete(removed.instanceId, revision: revision)
        default:
            return .ignore
        }
    }

    /// Joins a whole summary with the held one: every field comes from the incoming summary,
    /// except the status fields, which come from the summary with the greater `statusSeq`
    /// (the held one wins a tie).
    static func merge(held: InstanceSummary, incoming: InstanceSummary) -> InstanceSummary {
        incoming.statusSeq > held.statusSeq ? incoming : incoming.keepingStatus(of: held)
    }

    // MARK: - Calls

    /// One page of `instance.list`, after `token`, without retained instances.
    static func page(
        after token: PageToken?,
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> Page<InstanceSummary> {
        let params = ListInstancesParams(limit: pageLimit, before: token?.value)
        guard case .instanceList(let page) = try await call(.instanceList(params)) else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return Page(
            items: (page.items ?? []).filter { $0.lifecycle != .retained },
            next: page.next.map(PageToken.init))
    }

    /// One instance from `instance.get`, or `nil` when the daemon answers
    /// `resource.not_found` or the instance is retained.
    static func instance(
        _ id: UUID,
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> InstanceSummary? {
        let reply: Reply
        do throws(ClientError) {
            reply = try await call(.instanceGet(GetInstanceParams(instanceId: id)))
        } catch {
            if case .rejected(let body) = error, body.code == "resource.not_found" { return nil }
            throw error
        }
        guard case .instanceGet(let summary) = reply else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return summary.lifecycle == .retained ? nil : summary
    }
}

extension InstanceSummary {
    /// A copy with other status fields; every other field stays.
    private func replacing(
        readiness: InstanceReadiness? = nil,
        health: InstanceHealth? = nil,
        statusSeq: UInt64? = nil,
        process: InstanceProcess?? = nil,
        restart: InstanceRestart? = nil,
        blocker: InstanceBlocker?? = nil,
        logsUnavailable: Bool? = nil
    ) -> InstanceSummary {
        InstanceSummary(
            id: id, name: name, displayName: displayName, revision: revision,
            software: software, provider: provider, runtimeId: runtimeId,
            runtimeVersion: runtimeVersion, lifecycle: lifecycle, desired: desired,
            readiness: readiness ?? self.readiness, health: health ?? self.health,
            statusSeq: statusSeq ?? self.statusSeq, process: process ?? self.process,
            lastFailure: lastFailure, restart: restart ?? self.restart,
            blocker: blocker ?? self.blocker, endpoints: endpoints,
            leftoverProcesses: leftoverProcesses,
            logsUnavailable: logsUnavailable ?? self.logsUnavailable, ownership: ownership,
            initialized: initialized)
    }

    /// A copy with another `statusSeq`.
    func with(statusSeq: UInt64) -> InstanceSummary {
        replacing(statusSeq: statusSeq)
    }

    /// A copy with the status fields of an `instance.health_changed` event.
    func applying(_ change: InstanceHealthChanged) -> InstanceSummary {
        replacing(
            readiness: change.readiness, health: change.health, statusSeq: change.statusSeq,
            process: .some(change.process), restart: change.restart,
            blocker: .some(change.blocker), logsUnavailable: change.logsUnavailable)
    }

    /// A copy with the status fields of `other`.
    func keepingStatus(of other: InstanceSummary) -> InstanceSummary {
        replacing(
            readiness: other.readiness, health: other.health, statusSeq: other.statusSeq,
            process: .some(other.process), restart: other.restart,
            blocker: .some(other.blocker), logsUnavailable: other.logsUnavailable)
    }
}
