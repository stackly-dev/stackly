import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The Needs Attention rules (spec 0022 AC-3), read from the shared files
/// `list-mixed.json` and `attention-operations.json`.
struct AttentionRulesTests {
    private let locale = Locale(identifier: "en_US")
    private let timeZone = TimeZone(identifier: "UTC") ?? .gmt
    private let now = Date(timeIntervalSince1970: 1_792_486_800)

    private func items(
        instances: [InstanceSummary]? = nil, operations: [OperationSummary] = [],
        state: AttentionOperations? = nil,
        capacity: CapacityState = CapacityState(attempted: true)
    ) throws -> [AttentionItem] {
        AttentionRules.items(
            instances: try instances ?? InstanceContractFixture.mixed(),
            operations: state ?? .loaded(operations),
            runtimes: .none, capacity: capacity, now: now, locale: locale, timeZone: timeZone)
    }

    private func item(_ name: String, in items: [AttentionItem]) throws -> AttentionItem? {
        let id = try InstanceContractFixture.instance(name).id
        return items.first { $0.id == "instance:\(id.uuidString.lowercased())" }
    }

    /// `text` with the narrow no-break space of the macOS time format as a plain space.
    private static func plain(_ text: String) -> String {
        text.replacingOccurrences(of: "\u{202F}", with: " ")
    }

    // MARK: - One rule per instance

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected: "Each instance rule gives its tone, title, and body from the shared list"))
    func eachInstanceRuleGivesItsItem() throws {
        let all = try items()
        func check(_ name: String, rule: String, tone: Tone, title: String, message: String) throws
        {
            let found = try #require(try item(name, in: all), "no item for \(name)")
            #expect(found.rule == rule, "\(name)")
            #expect(found.tone == tone, "\(name)")
            #expect(found.title == title, "\(name)")
            #expect(found.message == message, "\(name)")
        }
        try check(
            "broken-db", rule: "I1", tone: .critical, title: "broken-db failed",
            message: "Crash loop after 5 restarts.")
        try check(
            "half-made", rule: "I1", tone: .critical, title: "half-made failed",
            message: "Initialization failed.")
        try check(
            "mystery", rule: "I2", tone: .caution, title: "Can’t confirm mystery’s process",
            message: "Stackly won’t start or stop it until the process is confirmed or gone.")
        try check(
            "locked-db", rule: "I3", tone: .critical, title: "locked-db’s data is in use",
            message: "Another process holds its data lock. Stop that process, then start locked-db."
        )
        try check(
            "secretive", rule: "I4", tone: .caution, title: "secretive needs a secret",
            message: "Store the missing secret, then start it again.")
        try check(
            "sick-db", rule: "I5", tone: .critical, title: "sick-db is unhealthy",
            message: "replication lag over 30 seconds")
        try check(
            "cache", rule: "I6", tone: .caution, title: "cache isn’t ready",
            message: "connection refused")
        try check(
            "slow-db", rule: "I8", tone: .caution, title: "slow-db is degraded",
            message: "slow queries")
        try check(
            "quiet-db", rule: "I9", tone: .caution, title: "Readiness of quiet-db is unknown",
            message: "No readiness check has answered recently.")
        try check(
            "stray-db", rule: "I10", tone: .neutral, title: "stray-db can’t be adopted again",
            message:
                "Its process doesn’t hold the instance lock. If stacklyd restarts, Stackly will mark it Unknown."
        )
    }

    @Test func aProblemWithoutADetailUsesTheDefaultSentence() throws {
        let sick = try InstanceContractFixture.instance("sick-db")
        let bare = InstanceSummary(
            id: sick.id, name: sick.name, displayName: nil, revision: sick.revision,
            software: sick.software, provider: sick.provider, runtimeId: sick.runtimeId,
            runtimeVersion: sick.runtimeVersion, lifecycle: .running, desired: .running,
            readiness: sick.readiness,
            health: InstanceHealth(
                state: .unhealthy, reason: "probe_failed", detail: nil,
                observedAt: sick.health.observedAt),
            statusSeq: sick.statusSeq, process: sick.process, lastFailure: nil,
            restart: sick.restart, blocker: nil, endpoints: nil, leftoverProcesses: nil,
            logsUnavailable: false, ownership: .manual, initialized: true)
        #expect(try items(instances: [bare]).first?.message == "Its health check is failing.")
    }

    @Test func aRestartingInstanceNamesTheAttemptAndTheTime() throws {
        let all = try items()
        let found = try #require(try item("flapping", in: all))
        #expect(found.rule == "I7")
        #expect(found.title == "flapping is restarting")
        // macOS puts a narrow no-break space before “AM”.
        #expect(Self.plain(found.message) == "It crashed. Restart 2 of 5 at 9:00 AM.")
    }

    @Test func aRestartWithoutATimeOrBudgetUsesThePlainSentence() throws {
        let base = try InstanceContractFixture.instance("flapping")
        func with(_ restart: InstanceRestart) -> InstanceSummary {
            InstanceSummary(
                id: base.id, name: base.name, displayName: nil, revision: base.revision,
                software: base.software, provider: base.provider, runtimeId: base.runtimeId,
                runtimeVersion: base.runtimeVersion, lifecycle: base.lifecycle,
                desired: base.desired, readiness: base.readiness, health: base.health,
                statusSeq: base.statusSeq, process: nil, lastFailure: nil, restart: restart,
                blocker: nil, endpoints: nil, leftoverProcesses: nil, logsUnavailable: false,
                ownership: .manual, initialized: true)
        }
        let plain = "It crashed and Stackly is starting it again."
        let noTime = InstanceRestart(
            attempts: 1, budget: 5, windowSeconds: 60, nextAttemptAt: nil, exhausted: false)
        let spent = InstanceRestart(
            attempts: 5, budget: 5, windowSeconds: 60, nextAttemptAt: now, exhausted: false)
        #expect(try items(instances: [with(noTime)]).first?.message == plain)
        #expect(try items(instances: [with(spent)]).first?.message == plain)
    }

    @Test func aTimeThatIsNotTodayGetsItsDate() throws {
        let base = try InstanceContractFixture.instance("flapping")
        let tomorrow = now.addingTimeInterval(86_400)
        let restart = InstanceRestart(
            attempts: 0, budget: 5, windowSeconds: 60, nextAttemptAt: tomorrow, exhausted: false)
        let summary = InstanceSummary(
            id: base.id, name: base.name, displayName: nil, revision: base.revision,
            software: base.software, provider: base.provider, runtimeId: base.runtimeId,
            runtimeVersion: base.runtimeVersion, lifecycle: .recovering, desired: base.desired,
            readiness: base.readiness, health: base.health, statusSeq: base.statusSeq,
            process: nil, lastFailure: nil, restart: restart, blocker: nil, endpoints: nil,
            leftoverProcesses: nil, logsUnavailable: false, ownership: .manual, initialized: true)
        let message = try #require(try items(instances: [summary]).first?.message)
        #expect(Self.plain(message) == "It crashed. Restart 1 of 5 at 10/21/2026, 9:00 AM.")
    }

    @Test func anInstanceThatMatchesSeveralRulesGivesTheSmallestNumberOnly() throws {
        let all = try items()
        // double-trouble is failed (I1) and also needs a secret (I4).
        let found = try #require(try item("double-trouble", in: all))
        #expect(found.rule == "I1")
        let id = try InstanceContractFixture.instance("double-trouble").id
        #expect(all.filter { $0.id == "instance:\(id.uuidString.lowercased())" }.count == 1)
    }

    @Test func aReadinessThatIsOnlyStaleAfterADaemonRestartIsNotAnItem() throws {
        let all = try items()
        for name in [
            "restarted-db", "queue", "starting-db", "stopping-db", "fresh-db", "raw-db",
            "orders-db", "analytics-db",
        ] {
            #expect(try item(name, in: all) == nil, "\(name)")
        }
    }

    // MARK: - Operations

    @Test func anOperationThatNeedsRecoveryGivesItsItemByHint() throws {
        let operations = try InstanceContractFixture.operations()
        let all = try items(instances: [], operations: operations)
        let byRule = all.filter { $0.rule == "O1" }
        #expect(byRule.count == 4)
        let messages = byRule.map(\.message)
        #expect(
            messages.contains(
                "It can continue. Resume or abandon it with stackly operation resume or abandon."))
        #expect(
            messages.contains("It can’t continue. Abandon it with stackly operation abandon."))
        #expect(
            messages.contains("It needs manual cleanup. Check it with stackly operation inspect."))
        #expect(messages.contains("Check it with stackly operation inspect."))
        #expect(byRule.allSatisfy { $0.tone == .critical && $0.destination == .operations })
        #expect(byRule.contains { $0.title == "Install runtime needs recovery" })
        #expect(byRule.contains { $0.title == "Create instance needs recovery" })
    }

    @Test func anOperationTheDaemonCompensatesAloneIsNotAnItem() throws {
        let operations = try InstanceContractFixture.operations()
        let all = try items(instances: [], operations: operations)
        #expect(all.contains { $0.title.hasPrefix("Start") } == false)
        #expect(all.count == 4)
    }

    // MARK: - Disk

    @Test(
        .evidence(
            covers: ["NFR-012"], expected: "Each disk level gives its item or none"))
    func eachDiskLevelGivesItsItem() throws {
        func disk(_ level: CapacityLevel, available: UInt64) throws -> AttentionItem? {
            let report = InstanceContractFixture.capacity(
                level, total: 994_662_584_320, available: available)
            return try items(
                instances: [], capacity: CapacityState(report: report, attempted: true)
            ).first
        }
        let critical = try #require(try disk(.critical, available: 1_700_000_000))
        #expect(critical.rule == "S1")
        #expect(critical.tone == .critical)
        #expect(critical.title == "Disk is almost full")
        #expect(
            critical.message
                == "Only 1.7 GB free on the disk that holds Stackly data. Instances can fail when it fills up."
        )
        #expect(critical.destination == .diagnostics)

        let low = try #require(try disk(.low, available: 4_100_000_000))
        #expect(low.rule == "S2")
        #expect(low.tone == .caution)
        #expect(
            low.message
                == "About 4.1 GB free of 994.66 GB on the disk that holds Stackly data.")

        let filling = try #require(try disk(.filling, available: 150_000_000_000))
        #expect(filling.rule == "S3")
        #expect(filling.tone == .neutral)
        #expect(filling.title == "Disk is 84% full")
        #expect(filling.message == "About 150 GB free on the disk that holds Stackly data.")

        #expect(try disk(.ok, available: 500_000_000_000) == nil)

        let unknown = try #require(try disk(.unrecognized, available: 500_000_000_000))
        #expect(unknown.rule == "S4")
        #expect(unknown.title == "Disk state not recognized")
        #expect(unknown.message == "Update Stackly to see this disk state.")
    }

    @Test func aDiskThatCouldNotBeReadGivesTheUnavailableItemOnlyWithoutAReport() throws {
        let error = ErrorBody(
            code: "storage.capacity_unavailable", category: .unavailable,
            message: "disk space for the data folder is unavailable", retryable: true,
            resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
            items: nil)
        let failed = try #require(
            try items(instances: [], capacity: CapacityState(failure: error, attempted: true)).first
        )
        #expect(failed.rule == "S4")
        #expect(failed.title == "Disk space unavailable")
        #expect(failed.message == "Disk space for the data folder is unavailable.")

        let unsupported = try #require(
            try items(instances: [], capacity: CapacityState(unsupported: true, attempted: true))
                .first)
        #expect(unsupported.message == "This version of stacklyd doesn’t report disk space.")

        let report = InstanceContractFixture.capacity(.ok)
        let kept = try items(
            instances: [], capacity: CapacityState(report: report, failure: error, attempted: true))
        #expect(kept.isEmpty)

        #expect(try items(instances: [], capacity: CapacityState()).isEmpty)
    }

    // MARK: - Order

    @Test func theItemsAreOrderedByToneCategoryRuleAndName() throws {
        let report = InstanceContractFixture.capacity(.critical, available: 1_000_000_000)
        let all = try items(
            operations: try InstanceContractFixture.operations(),
            capacity: CapacityState(report: report, attempted: true))
        let tones = all.map { $0.tone }
        func rank(_ tone: Tone) -> Int { tone == .critical ? 0 : tone == .caution ? 1 : 2 }
        #expect(tones.map(rank) == tones.map(rank).sorted())
        #expect(all.first?.id == "disk")
        let critical = all.filter { $0.tone == .critical }
        #expect(critical.map(\.category).first == .disk)
        let ranksByCategory = critical.map(\.category.rawValue)
        #expect(ranksByCategory == ranksByCategory.sorted())
        let instanceCritical = critical.filter { $0.category == .instance }
        #expect(instanceCritical.map(\.ruleNumber) == instanceCritical.map(\.ruleNumber).sorted())
    }

    @Test func theIDsAreStableAndTheSameInputGivesTheSameItems() throws {
        #expect(try items() == (try items()))
        let all = try items(operations: try InstanceContractFixture.operations())
        #expect(Set(all.map(\.id)).count == all.count)
        #expect(
            all.allSatisfy {
                $0.id.hasPrefix("instance:") || $0.id.hasPrefix("operation:") || $0.id == "disk"
            })
    }

    @Test func aDiskItemAboveSixFailedInstancesStaysFirst() throws {
        let failed = (1...7).map {
            InstanceSummary.fixture(number: UInt8($0), name: "db-\($0)", lifecycle: .failed)
        }
        let report = InstanceContractFixture.capacity(.critical, available: 1_000_000_000)
        let all = try items(
            instances: failed, capacity: CapacityState(report: report, attempted: true))
        #expect(all.count == 8)
        #expect(all.first?.id == "disk")
    }

    @Test func theSpokenLabelStartsWithTheLevel() throws {
        let all = try items()
        let critical = try #require(all.first { $0.tone == .critical })
        #expect(critical.spokenLabel.hasPrefix("Critical: "))
        let caution = try #require(all.first { $0.tone == .caution })
        #expect(caution.spokenLabel.hasPrefix("Warning: "))
        let neutral = try #require(all.first { $0.tone == .neutral })
        #expect(neutral.spokenLabel.hasPrefix("Info: "))
        #expect(critical.spokenHint.hasPrefix("Opens "))
    }
}
