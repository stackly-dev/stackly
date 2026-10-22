import Foundation
import StacklyClient

/// What Needs Attention knows of the operations (spec 0022, *Aturan Needs Attention*, 2026-10-22).
///
/// There is no case for operations that have not loaded: Needs Attention waits for them
/// before it reads anything, so the rules never see that state.
nonisolated public enum AttentionOperations: Sendable, Equatable {
    /// The operations that have not ended, from a snapshot that loaded; a later refresh that
    /// failed keeps them.
    case loaded([OperationSummary])
    /// The snapshot failed and nothing was ever loaded. It carries no data: an operation that
    /// arrives by event after a failed snapshot is not an item, because without the snapshot
    /// the list is not complete, and item O2 says so.
    case failed
    /// The daemon does not know `operation.list`.
    case unsupported
}

/// The rules of Needs Attention (spec 0022, *Aturan Needs Attention*): a pure function of
/// what the stores hold, so the same input gives the same items with the same IDs.
///
/// An instance gives at most one item, from the first rule (the smallest number) that
/// matches. Items are ordered by tone (critical, caution, neutral), then by category (disk,
/// operation, instance), then by rule number, then by the sort key of the rule.
nonisolated public enum AttentionRules {
    /// The items.
    ///
    /// - Parameters:
    ///   - instances: Every instance in the store.
    ///   - operations: What is known of the operations: the ones that have not ended (rule
    ///     O1 reads those that need recovery), or that they cannot be read (rule O2).
    ///   - runtimes: The runtime names the operation titles use.
    ///   - capacity: What the disk reading holds.
    ///   - now: The clock; a time that is not today gets its date.
    ///   - locale: The locale of the times and sizes.
    ///   - timeZone: The time zone of the times.
    public static func items(
        instances: [InstanceSummary],
        operations: AttentionOperations,
        runtimes: OperationTitle.RuntimeNames = .none,
        capacity: CapacityState,
        now: Date,
        locale: Locale,
        timeZone: TimeZone
    ) -> [AttentionItem] {
        let names = Dictionary(
            instances.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first })
        var items: [AttentionItem] = instances.compactMap {
            instanceItem($0, now: now, locale: locale, timeZone: timeZone)
        }
        switch operations {
        case .loaded(let active):
            items += active.compactMap {
                operationItem($0, instanceNames: names, runtimes: runtimes)
            }
        case .failed:
            items.append(
                unreadableOperations(
                    "Stackly couldn’t read operations. Any that need recovery are missing from this list."
                ))
        case .unsupported:
            items.append(
                unreadableOperations(
                    "This version of stacklyd doesn’t list operations. Any that need recovery are missing from this list."
                ))
        }
        if let disk = diskItem(capacity, locale: locale) {
            items.append(disk)
        }
        return items.sorted(by: ordered)
    }

    /// The banner the inspector shows for an instance: the item of rules I1 to I4, the
    /// problems a person can fix, or `nil` for an instance whose item comes from another
    /// rule or who has none.
    ///
    /// - Parameters:
    ///   - instance: The instance.
    ///   - now: The clock.
    ///   - locale: The locale of the texts.
    ///   - timeZone: The time zone of the texts.
    public static func banner(
        for instance: InstanceSummary, now: Date, locale: Locale, timeZone: TimeZone
    ) -> AttentionItem? {
        guard let item = instanceItem(instance, now: now, locale: locale, timeZone: timeZone),
            ["I1", "I2", "I3", "I4"].contains(item.rule)
        else { return nil }
        return item
    }

    // MARK: - Order

    private static func toneRank(_ tone: Tone) -> Int {
        switch tone {
        case .critical: 0
        case .caution: 1
        case .neutral: 2
        case .positive: 3
        case .informative: 4
        }
    }

    private static func ordered(_ a: AttentionItem, _ b: AttentionItem) -> Bool {
        if toneRank(a.tone) != toneRank(b.tone) { return toneRank(a.tone) < toneRank(b.tone) }
        if a.category != b.category { return a.category < b.category }
        if a.ruleNumber != b.ruleNumber { return a.ruleNumber < b.ruleNumber }
        switch a.sortKey.localizedStandardCompare(b.sortKey) {
        case .orderedAscending: return true
        case .orderedDescending: return false
        case .orderedSame: return a.id < b.id
        }
    }

    // MARK: - Instances

    private static func instanceItem(
        _ instance: InstanceSummary, now: Date, locale: Locale, timeZone: TimeZone
    ) -> AttentionItem? {
        let name = instance.name
        func item(
            _ rule: String, _ tone: Tone, _ title: String, _ message: String
        ) -> AttentionItem {
            AttentionItem(
                id: "instance:\(instance.id.uuidString.lowercased())", rule: rule, tone: tone,
                category: .instance, title: title, message: message,
                destination: .instance(instance.id), sortKey: name)
        }
        let isRunning = instance.lifecycle == .running
        let readiness = instance.readiness
        let health = instance.health
        if instance.lifecycle == .failed {
            return item(
                "I1", .critical, "\(name) failed", InstanceFailureCopy.text(for: instance))
        }
        if instance.lifecycle == .unknown || Self.isIdentityUnproven(instance.blocker) {
            return item(
                "I2", .caution, "Can’t confirm \(name)’s process",
                "Stackly won’t start or stop it until the process is confirmed or gone.")
        }
        if case .dataInUse = instance.blocker {
            return item(
                "I3", .critical, "\(name)’s data is in use",
                "Another process holds its data lock. Stop that process, then start \(name).")
        }
        if case .needsSecret = instance.blocker {
            return item(
                "I4", .caution, "\(name) needs a secret",
                "Store the missing secret, then start it again.")
        }
        if isRunning, readiness.state == .ready, health.state == .unhealthy {
            return item(
                "I5", .critical, "\(name) is unhealthy",
                health.detail ?? "Its health check is failing.")
        }
        if isRunning, readiness.state == .notReady,
            readiness.reason != StatusAdapter.awaitingProbeReason
        {
            return item(
                "I6", .caution, "\(name) isn’t ready",
                readiness.detail ?? "Its readiness check is failing.")
        }
        if instance.lifecycle == .recovering {
            return item(
                "I7", .caution, "\(name) is restarting",
                restartMessage(instance.restart, now: now, locale: locale, timeZone: timeZone))
        }
        if isRunning, readiness.state == .ready, health.state == .degraded {
            return item(
                "I8", .caution, "\(name) is degraded",
                health.detail ?? "Its health check reports a problem.")
        }
        if isRunning, readiness.state == .unknown, readiness.reason == "stale" {
            return item(
                "I9", .caution, "Readiness of \(name) is unknown",
                "No readiness check has answered recently.")
        }
        if isRunning, instance.process?.adoptable == false {
            return item(
                "I10", .neutral, "\(name) can’t be adopted again",
                "Its process doesn’t hold the instance lock. If stacklyd restarts, Stackly will mark it Unknown."
            )
        }
        return nil
    }

    private static func isIdentityUnproven(_ blocker: InstanceBlocker?) -> Bool {
        if case .identityUnproven = blocker { return true }
        return false
    }

    /// "It crashed. Restart {attempts + 1} of {budget} at {time}." while a restart is
    /// planned inside the budget, else the plain sentence.
    private static func restartMessage(
        _ restart: InstanceRestart, now: Date, locale: Locale, timeZone: TimeZone
    ) -> String {
        guard let next = restart.nextAttemptAt, restart.attempts + 1 <= restart.budget else {
            return "It crashed and Stackly is starting it again."
        }
        var calendar = locale.calendar
        calendar.timeZone = timeZone
        let style = Date.FormatStyle(
            date: calendar.isDate(next, inSameDayAs: now) ? .omitted : .numeric,
            time: .shortened, locale: locale, calendar: calendar, timeZone: timeZone)
        return
            "It crashed. Restart \(restart.attempts + 1) of \(restart.budget) at \(next.formatted(style))."
    }

    // MARK: - Operations

    private static func operationItem(
        _ operation: OperationSummary, instanceNames: [UUID: String],
        runtimes: OperationTitle.RuntimeNames
    ) -> AttentionItem? {
        guard operation.state == .needsRecovery else { return nil }
        let hint = operation.recovery?.hint ?? .unrecognized
        if hint == .autoCompensate { return nil }
        // provisional: F35 (the Operations page replaces these words with a link to it)
        let message: String =
            switch hint {
            case .resume:
                "It can continue. Resume or abandon it with stackly operation resume or abandon."
            case .abandon:
                "It can’t continue. Abandon it with stackly operation abandon."
            case .manual:
                "It needs manual cleanup. Check it with stackly operation inspect."
            case .autoCompensate, .unrecognized:
                "Check it with stackly operation inspect."
            }
        let title = OperationTitle.title(
            for: operation, instanceNames: instanceNames, runtimes: runtimes)
        return AttentionItem(
            id: "operation:\(operation.id.uuidString.lowercased())", rule: "O1", tone: .critical,
            category: .operation, title: "\(title) needs recovery", message: message,
            destination: .operations, sortKey: title)
    }

    /// Rule O2: the operations cannot be read, so the ones that need recovery may be missing.
    private static func unreadableOperations(_ message: String) -> AttentionItem {
        AttentionItem(
            id: "operations", rule: "O2", tone: .neutral, category: .operation,
            title: "Operations unavailable", message: message, destination: .operations,
            sortKey: "")
    }

    // MARK: - Disk

    private static func diskItem(_ capacity: CapacityState, locale: Locale) -> AttentionItem? {
        func item(_ rule: String, _ tone: Tone, _ title: String, _ message: String)
            -> AttentionItem
        {
            AttentionItem(
                id: "disk", rule: rule, tone: tone, category: .disk, title: title,
                message: message, destination: .diagnostics, sortKey: "")
        }
        if let report = capacity.report {
            let available = CatalogCopy.sizeText(report.availableBytes, locale: locale)
            switch report.level {
            case .ok:
                return nil
            case .critical:
                return item(
                    "S1", .critical, "Disk is almost full",
                    "Only \(available) free on the disk that holds Stackly data. Instances can fail when it fills up."
                )
            case .low:
                return item(
                    "S2", .caution, "Disk space is low",
                    "About \(available) free of \(CatalogCopy.sizeText(report.totalBytes, locale: locale)) on the disk that holds Stackly data."
                )
            case .filling:
                return item(
                    "S3", .neutral, "Disk is \(report.usedPercent)% full",
                    "About \(available) free on the disk that holds Stackly data.")
            case .unrecognized:
                return item(
                    "S4", .neutral, "Disk state not recognized",
                    "Update Stackly to see this disk state.")
            }
        }
        if capacity.unsupported {
            return item(
                "S4", .neutral, "Disk space unavailable",
                "This version of stacklyd doesn’t report disk space.")
        }
        if let failure = capacity.failure {
            return item(
                "S4", .neutral, "Disk space unavailable", DaemonText.sentence(failure.message))
        }
        return nil
    }
}
