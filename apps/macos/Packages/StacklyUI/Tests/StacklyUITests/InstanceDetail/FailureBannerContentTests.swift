import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The content of the failure banner (spec 0033, *State banner*, *Pemetaan sebab*, *Copy
/// banner*, and *Operasi terkait*): one case per rule F1 to F7, the order of the rules, which
/// operation the banner reads, and the words built from numbers and details.
@MainActor
struct FailureBannerContentTests {
    private let now = FixtureClock.now

    private static let names: [UUID: String] = Dictionary(
        (FixtureScenario.instanceCases + FixtureFailureBanner.instances).map { ($0.id, $0.name) },
        uniquingKeysWith: { first, _ in first })

    private func resolve(
        _ summary: InstanceSummary, _ related: [OperationSummary], now: Date? = nil
    ) -> FailureBannerContent? {
        FailureBannerContent.resolve(
            summary, related: related, instanceNames: { Self.names[$0] }, now: now ?? self.now)
    }

    private func resolve(_ fixture: FixtureFailureBanner) throws -> FailureBannerContent {
        try #require(resolve(fixture.instance, fixture.operations))
    }

    private static func operationID(_ number: UInt8) -> String {
        FixtureID.uuid(kind: FixtureID.operation, number: number).uuidString.lowercased()
    }

    // MARK: - Rules F1 to F7

    @Test(
        .evidence(
            covers: ["AC-068"],
            expected:
                "A recovering instance shows the caution banner with the restart that waits, its countdown, the excerpt, and Show Logs"
        ))
    func aRecoveringInstanceShowsTheRestartThatWaits() throws {
        // covers: spec 0033 AC-12 (the restarting banner)
        let content = try resolve(.restarting)
        #expect(content.rule == .restarting)
        #expect(content.tone == .caution)
        #expect(content.title == "Restarting after a crash")
        #expect(content.message == "Restart 2 of 5 in 4 s.")
        #expect(content.codeLine == nil)
        #expect(content.excerpt.count == 2)
        #expect(content.actions == [.showLogs])
        #expect(content.operationId == nil)
    }

    @Test func anInstanceThatNeverInitializedShowsInitializationFailedWithoutRetry() throws {
        let content = try resolve(.initFailed)
        #expect(content.rule == .initFailed)
        #expect(content.tone == .critical)
        #expect(content.title == "Initialization failed")
        #expect(content.message == "Initialization failed.")
        #expect(content.codeLine == "init_step_failed")
        #expect(content.actions == [.showLogs, .diagnose])
    }

    @Test(
        .evidence(
            covers: ["AC-068", "NFR-011", "VAL-020"],
            expected:
                "A crash loop shows its title, the crash count and window, the hint, the code with the operation ID, the excerpt, and Retry first"
        ))
    func aCrashLoopShowsTheCountTheCodeAndTheOperation() throws {
        // covers: spec 0033 AC-9 (the crash loop banner)
        let content = try resolve(.crashLoop)
        #expect(content.rule == .crashLoop)
        #expect(content.title == "Crash loop. Automatic restarts paused.")
        #expect(
            content.message
                == "It crashed 6 times in 5 minutes, so Stackly stopped restarting it and is waiting for you."
        )
        #expect(
            content.hint
                == "It kept crashing, so Stackly stopped restarting it. Read its logs, fix the cause, then start it again."
        )
        #expect(
            content.codeLine
                == "supervisor.retry_budget_exhausted · operation \(Self.operationID(41))")
        #expect(content.operationId?.uuidString.lowercased() == Self.operationID(41))
        #expect(
            content.summaryCode
                == """
                supervisor.retry_budget_exhausted · operation \(Self.operationID(41))
                [ERR] Error parsing config file: unknown field "jetstream_max"
                [FTL] Server exiting
                """)
        #expect(content.actions == [.retry, .showLogs, .diagnose])
    }

    @Test(
        .evidence(
            covers: ["AC-010", "AC-055"],
            expected:
                "A start that failed on a port held by another process offers Use Port with the suggested port first"
        ))
    func aPortInUseOffersTheSuggestedPort() throws {
        // covers: spec 0033 AC-10 (Use Port with the suggested port)
        let content = try resolve(.portInUse)
        let fix = PortFix(endpoint: "database", port: 5439)
        #expect(content.rule == .port)
        #expect(content.title == "Couldn’t start: port in use")
        #expect(content.message == "Port 5432 of database is held by another process.")
        #expect(content.hint?.hasPrefix("Stackly never moves a fixed port on its own.") == true)
        #expect(content.portFix == fix)
        #expect(content.actions == [.usePort(fix), .showLogs, .retry, .diagnose])
        #expect(content.actions.first?.title == "Use Port 5439")
        #expect(content.inspectorActions == [.usePort(fix)])
        #expect(content.inspectorCode?.contains("operation") == false)
    }

    @Test func aPortProblemFromTheSummaryAloneOffersChangePortWithoutAnOperation() throws {
        // covers: spec 0033 AC-10 (Change Port… without a suggestion)
        let content = try resolve(.portWithoutOperation)
        #expect(content.rule == .port)
        #expect(content.title == "Couldn’t start: port in use")
        #expect(content.message == "Another process uses one of its ports.")
        #expect(content.codeLine == "endpoint_in_use")
        #expect(content.operationId == nil)
        #expect(content.portFix == nil)
        #expect(content.actions == [.changePort, .showLogs, .retry, .diagnose])
        #expect(content.actions.first?.title == "Change Port…")
    }

    @Test(
        .evidence(
            covers: ["AC-010"],
            expected:
                "A stopped instance whose start was refused because another instance owns the port names that instance and offers its free port"
        ))
    func aPortOfAnotherInstanceNamesItOnAStoppedInstance() throws {
        let content = try resolve(.portConflict)
        #expect(content.rule == .port)
        #expect(content.title == "Couldn’t start: port in use")
        #expect(content.message == "Port 5432 of database belongs to pg17-dev.")
        #expect(content.portFix == PortFix(endpoint: "database", port: 5441))
        #expect(content.excerpt.isEmpty)
        #expect(content.inspectorCode == "supervisor.endpoint_conflict")
    }

    @Test func aPortThatIsGoneSaysAnotherInstance() throws {
        let fixture = FixtureFailureBanner.portConflict
        let content = try #require(
            FailureBannerContent.resolve(
                fixture.instance, related: fixture.operations, instanceNames: { _ in nil },
                now: now))
        #expect(content.message == "Port 5432 of database belongs to another instance.")
    }

    @Test func aPortMacOSRefusedHasItsOwnTitleAndNoSuggestion() throws {
        let content = try resolve(.portUnavailable)
        #expect(content.title == "Couldn’t start: port unavailable")
        #expect(content.message == "macOS refused port 5442 of database.")
        #expect(content.hint == "Pick another port for this endpoint.")
        #expect(content.actions == [.changePort, .showLogs, .retry, .diagnose])
    }

    @Test(
        .evidence(
            covers: ["AC-010"],
            expected:
                "After a port change succeeded on a failed instance the banner hides the port button and asks for Retry"
        ))
    func aSuccessfulPortChangeHidesThePortButtonAndAsksForRetry() throws {
        // covers: spec 0033 AC-10 (after a port change)
        let content = try resolve(.portsChanged)
        #expect(content.rule == .port)
        #expect(content.portsChanged)
        #expect(content.portFix == nil)
        #expect(content.actions == [.retry, .showLogs, .diagnose])
        #expect(content.message == "Port 5444 of database is held by another process.")
        #expect(content.operationId?.uuidString.lowercased() == Self.operationID(45))
        #expect(
            FailureBannerContent.portsChangedNote == "Its ports changed. Retry to start with them.")
    }

    @Test func dataInUseNamesTheLockAndKeepsTheHint() throws {
        let content = try resolve(.dataInUse)
        #expect(content.rule == .dataInUse)
        #expect(content.title == "Couldn’t start: data in use")
        #expect(content.message == "Another process holds its data lock.")
        #expect(content.hint?.hasPrefix("Stackly never removes the lock on its own.") == true)
        #expect(content.actions == [.retry, .showLogs, .diagnose])
    }

    @Test func anotherFailureUsesTheSentenceOfSpec0022() throws {
        // covers: spec 0033 AC-11 (another cause uses InstanceFailureCopy)
        let content = try resolve(.failed)
        #expect(content.rule == .failed)
        #expect(content.title == "warehouse failed")
        #expect(content.message == "Stopped by signal 9.")
        #expect(content.codeLine == "process_signaled")
        #expect(content.actions == [.retry, .showLogs, .diagnose])
    }

    @Test(
        .evidence(
            covers: ["VAL-020"],
            expected:
                "A stopped instance whose start the daemon refused before its journal shows the error sentence and its operation"
        ))
    func aRefusedStartOfAStoppedInstanceShowsTheError() throws {
        // covers: spec 0033 AC-11 (a refused start of a stopped instance)
        let content = try resolve(.startRefused)
        #expect(content.rule == .startRefused)
        #expect(content.title == "Couldn’t start inventory-db")
        #expect(content.message == "Runtime postgresql 17.6 is not installed.")
        #expect(content.hint == nil)
        #expect(content.excerpt.isEmpty)
        #expect(
            content.codeLine == "supervisor.runtime_unavailable · operation \(Self.operationID(48))"
        )
        #expect(content.actions == [.retry, .showLogs, .diagnose])
    }

    @Test func aCodeThisBuildDoesNotKnowFallsBackToItsName() throws {
        let summary = FixtureFailureBanner.failed.instance.banner(
            lastFailure: InstanceLastFailure(
                code: "brand_new_reason", at: now, exitCode: nil, signal: nil, excerpt: nil))
        let content = try #require(resolve(summary, []))
        #expect(content.rule == .failed)
        #expect(content.message == "Failed with brand_new_reason.")
        #expect(content.codeLine == "brand_new_reason")
    }

    @Test func aFailureWithoutDataSaysOnlyThatItFailed() throws {
        let summary = FixtureFailureBanner.failed.instance.banner(lastFailure: .some(nil))
        let content = try #require(resolve(summary, []))
        #expect(content.message == "It failed.")
        #expect(content.codeLine == nil)
        #expect(content.summaryCode == nil)
    }

    // MARK: - Order

    @Test func initializationComesBeforeTheCause() throws {
        let summary = FixtureFailureBanner.crashLoop.instance.banner(initialized: false)
        let content = try #require(
            resolve(summary, FixtureFailureBanner.crashLoop.operations))
        #expect(content.rule == .initFailed)
    }

    @Test func aRecoveringInstanceNeverReadsAFailedOperation() throws {
        let fixture = FixtureFailureBanner.restarting
        let failed = FixtureFailureBanner.crashLoop.operations[0].moved(to: fixture.id)
        let content = try #require(resolve(fixture.instance, [failed]))
        #expect(content.rule == .restarting)
        #expect(content.operationId == nil)
    }

    @Test(
        .evidence(
            covers: ["VAL-020"],
            expected:
                "An instance that keeps banner I2 to I4 gets no failure banner, while a failed one with a blocker still does, as I1 did"
        ))
    func instancesWithTheBannersOfSpec0022GetNone() throws {
        let refused = FixtureFailureBanner.dataInUse.operations[0].moved(
            to: FixtureFailureBanner.startRefused.id)
        let blocked = FixtureFailureBanner.startRefused.instance.banner(
            blocker: FixtureScenario.instanceCases[6].blocker)
        #expect(resolve(blocked, [refused]) == nil)
        let unknown = FixtureFailureBanner.startRefused.instance.banner(lifecycle: .unknown)
        #expect(resolve(unknown, FixtureFailureBanner.startRefused.operations) == nil)
        let recoveringBlocked = FixtureFailureBanner.restarting.instance.banner(
            blocker: FixtureScenario.instanceCases[8].blocker)
        #expect(resolve(recoveringBlocked, []) == nil)
        let failedBlocked = FixtureFailureBanner.failed.instance.banner(
            blocker: FixtureScenario.instanceCases[8].blocker)
        #expect(resolve(failedBlocked, [])?.rule == .failed)
    }

    @Test func aRunningOrPlainStoppedInstanceGetsNone() {
        #expect(resolve(FixtureScenario.instanceCases[0], []) == nil)
        #expect(resolve(FixtureFailureBanner.startRefused.instance, []) == nil)
    }

    // MARK: - Related operation

    @Test(
        .evidence(
            covers: ["VAL-020"],
            expected:
                "A newer rename is passed over, while a newer stop, start, or port change hides the banner of a stopped instance"
        ))
    func newerOperationsHideTheBannerOfAStoppedInstanceExceptRename() throws {
        // covers: spec 0033 AC-11 (which newer operations hide the banner)
        let fixture = FixtureFailureBanner.startRefused
        let refused = fixture.operations
        func newer(_ kind: OperationKind, _ state: OperationState) -> OperationSummary {
            Self.operation(91, kind, state, on: fixture.id, endedAt: FixtureClock.secondsAgo(5))
        }
        #expect(
            resolve(fixture.instance, [newer(.renameInstance, .succeeded)] + refused)?.rule
                == .startRefused)
        #expect(
            resolve(fixture.instance, [newer(.applyConfig, .succeeded)] + refused)?.rule
                == .startRefused)
        #expect(resolve(fixture.instance, [newer(.stopInstance, .succeeded)] + refused) == nil)
        #expect(resolve(fixture.instance, [newer(.startInstance, .running)] + refused) == nil)
        #expect(resolve(fixture.instance, [newer(.changeEndpoints, .succeeded)] + refused) == nil)
        #expect(resolve(fixture.instance, [newer(.startInstance, .cancelled)] + refused) == nil)
    }

    @Test func theOrderIsNewestFirstWhateverOrderTheListCameIn() throws {
        let fixture = FixtureFailureBanner.portsChanged
        let content = try #require(resolve(fixture.instance, fixture.operations.reversed()))
        #expect(content.portsChanged)
        #expect(content.operationId?.uuidString.lowercased() == Self.operationID(45))
    }

    @Test func anOperationOfAnotherInstanceIsIgnored() {
        let fixture = FixtureFailureBanner.startRefused
        let other = fixture.operations[0].moved(to: FixtureFailureBanner.portConflict.id)
        #expect(resolve(fixture.instance, [other]) == nil)
    }

    @Test(
        .evidence(
            covers: ["AC-068"],
            expected:
                "An operation that ended before the recorded failure is not its error, so the banner shows the summary alone"
        ))
    func anOperationOlderThanTheFailureIsNotItsError() throws {
        let fixture = FixtureFailureBanner.crashLoop
        let older = fixture.instance.banner(
            lastFailure: InstanceLastFailure(
                code: "crash_loop", at: FixtureClock.secondsAgo(1), exitCode: 3, signal: nil,
                excerpt: nil))
        let content = try #require(resolve(older, fixture.operations))
        #expect(content.rule == .crashLoop)
        #expect(content.operationId == nil)
        #expect(content.hint == nil)
        #expect(content.codeLine == "crash_loop")
    }

    @Test(
        .evidence(
            covers: ["AC-068"],
            expected:
                "After a daemon restart the failure has no time, so the operation counts only when its code stands on the row of the failure code"
        ))
    func withoutATimeTheCodeRowDecides() throws {
        let fixture = FixtureFailureBanner.crashLoop
        let restarted = fixture.instance.banner(
            lastFailure: InstanceLastFailure(
                code: "crash_loop", at: nil, exitCode: nil, signal: nil, excerpt: nil),
            restart: InstanceRestart(
                attempts: 0, budget: 5, windowSeconds: 300, nextAttemptAt: nil, exhausted: false))
        let same = try #require(resolve(restarted, fixture.operations))
        #expect(same.operationId?.uuidString.lowercased() == Self.operationID(41))
        #expect(
            same.message
                == "It kept crashing, so Stackly stopped restarting it and is waiting for you.")

        let port = FixtureFailureBanner.portInUse.operations[0].moved(to: fixture.id)
        let other = try #require(resolve(restarted, [port]))
        #expect(other.rule == .crashLoop)
        #expect(other.operationId == nil)
    }

    @Test func codesTheTableDoesNotListShareOneRow() throws {
        let fixture = FixtureFailureBanner.failed
        let restarted = fixture.instance.banner(
            lastFailure: InstanceLastFailure(
                code: "spawn_failed", at: nil, exitCode: nil, signal: nil, excerpt: nil))
        let spawn = FixtureFailureBanner.startRefused.operations[0].moved(to: fixture.id)
        let content = try #require(resolve(restarted, [spawn]))
        #expect(content.rule == .failed)
        #expect(content.operationId?.uuidString.lowercased() == Self.operationID(48))
    }

    @Test(
        .evidence(
            covers: ["AC-068"],
            expected:
                "The banner rises from the summary alone to the full content once the operation arrives, and its title never changes"
        ))
    func theTitleStaysWhenTheOperationArrives() throws {
        for fixture in FixtureFailureBanner.allCases where fixture.instance.lifecycle != .stopped {
            let alone = try #require(resolve(fixture.instance, []))
            let full = try resolve(fixture)
            #expect(alone.title == full.title, "\(fixture)")
            #expect(alone.rule == full.rule, "\(fixture)")
            #expect(alone.operationId == nil, "\(fixture)")
            #expect(alone.portFix == nil, "\(fixture)")
        }
    }

    @Test func theOperationIDFallsBackToTheOperation() throws {
        let fixture = FixtureFailureBanner.startRefused
        let bare = try #require(fixture.operations.first).withoutOperationID
        let content = try #require(resolve(fixture.instance, [bare]))
        #expect(content.operationId?.uuidString.lowercased() == Self.operationID(48))
    }

    // MARK: - Words

    @Test func theCountdownRoundsUpAndNeverGoesBelowZero() throws {
        // covers: spec 0033 AC-12 (the countdown)
        let fixture = FixtureFailureBanner.restarting
        let next = try #require(fixture.instance.restart.nextAttemptAt)
        #expect(
            resolve(fixture.instance, [], now: next.addingTimeInterval(-3.2))?.message
                == "Restart 2 of 5 in 4 s.")
        #expect(
            resolve(fixture.instance, [], now: next.addingTimeInterval(2))?.message
                == "Restart 2 of 5 in 0 s.")
        let waiting = fixture.instance.banner(
            restart: InstanceRestart(
                attempts: 2, budget: 5, windowSeconds: 300, nextAttemptAt: nil, exhausted: false))
        #expect(resolve(waiting, [])?.message == "Stackly is starting it again.")
        let spent = fixture.instance.banner(
            restart: InstanceRestart(
                attempts: 6, budget: 5, windowSeconds: 300, nextAttemptAt: next, exhausted: false))
        #expect(resolve(spent, [])?.message == "Stackly is starting it again.")
    }

    @Test func theWindowIsInWholeMinutesRoundedUp() {
        #expect(FailureBannerCopy.window(300) == "5 minutes")
        #expect(FailureBannerCopy.window(60) == "1 minute")
        #expect(FailureBannerCopy.window(61) == "2 minutes")
        #expect(FailureBannerCopy.window(30) == "1 minute")
    }

    @Test func excerptLinesSplitOnNewlines() {
        let failure = InstanceLastFailure(
            code: "process_exited", at: nil, exitCode: 1, signal: nil,
            excerpt: ["first\nsecond", "third"])
        #expect(FailureBannerCopy.lines(of: failure) == ["first", "second", "third"])
        #expect(FailureBannerCopy.lines(of: nil).isEmpty)
    }

    @Test func eachCodeMapsToTheRowOfItsColumn() {
        #expect(FailureCause(failureCode: "crash_loop") == .crashLoop)
        #expect(FailureCause(errorCode: "supervisor.retry_budget_exhausted") == .crashLoop)
        #expect(FailureCause(failureCode: "endpoint_in_use") == .port(.inUse))
        #expect(FailureCause(errorCode: "supervisor.endpoint_in_use") == .port(.inUse))
        #expect(FailureCause(errorCode: "supervisor.endpoint_conflict") == .port(.conflict))
        #expect(FailureCause(errorCode: "supervisor.endpoint_unavailable") == .port(.unavailable))
        #expect(FailureCause(failureCode: "data_in_use") == .dataInUse)
        #expect(FailureCause(errorCode: "supervisor.data_in_use") == .dataInUse)
        #expect(FailureCause(failureCode: "process_exited") == .other("process_exited"))
        #expect(FailureCause(failureCode: "endpoint_conflict") == .other("endpoint_conflict"))
        #expect(FailureCause.other("a").isSameRow(as: .other("b")))
        #expect(FailureCause.crashLoop.isSameRow(as: .port(.inUse)) == false)
    }

    @Test func aSuggestionNeedsTheEndpointAndAPort() {
        func error(_ details: [(String, String)]) -> ErrorBody {
            ErrorBody(
                code: "supervisor.endpoint_in_use", category: .conflict, message: "taken",
                retryable: false, resource: nil, fields: nil, hint: nil,
                details: details.map { ErrorDetail(key: $0.0, value: $0.1) }, operationId: nil,
                rule: nil, items: nil)
        }
        #expect(
            FailureBannerCopy.portFix(of: error([("endpoint", "db"), ("suggestedPort", "5500")]))
                == PortFix(endpoint: "db", port: 5500))
        #expect(FailureBannerCopy.portFix(of: error([("suggestedPort", "5500")])) == nil)
        #expect(FailureBannerCopy.portFix(of: error([("endpoint", "db")])) == nil)
        #expect(
            FailureBannerCopy.portFix(of: error([("endpoint", "db"), ("suggestedPort", "x")]))
                == nil)
    }

    // MARK: - Pieces

    private static func operation(
        _ number: UInt8, _ kind: OperationKind, _ state: OperationState, on id: UUID,
        endedAt: Date
    ) -> OperationSummary {
        OperationSummary(
            id: FixtureID.uuid(kind: FixtureID.operation, number: number), kind: kind,
            resource: ResourceRef(kind: .instance, id: id.uuidString.lowercased()), state: state,
            stage: nil, progress: .indeterminate, waitingOn: nil, cancelRequestedAt: nil,
            pointOfNoReturnAt: nil, failure: nil, recovery: nil, result: nil,
            trigger: OperationTriggerInfo(origin: .user, client: .gui),
            createdAt: endedAt.addingTimeInterval(-1), startedAt: endedAt, updatedAt: endedAt,
            revision: 2, cancel: nil)
    }
}

extension InstanceSummary {
    /// A copy with other failure facts; every other field stays.
    fileprivate func banner(
        lifecycle: InstanceLifecycle? = nil,
        lastFailure: InstanceLastFailure?? = nil,
        restart: InstanceRestart? = nil,
        blocker: InstanceBlocker?? = nil,
        initialized: Bool? = nil
    ) -> InstanceSummary {
        InstanceSummary(
            id: id, name: name, displayName: displayName, revision: revision,
            software: software, provider: provider, runtimeId: runtimeId,
            runtimeVersion: runtimeVersion, lifecycle: lifecycle ?? self.lifecycle,
            desired: desired, readiness: readiness, health: health, statusSeq: statusSeq,
            process: process, lastFailure: lastFailure ?? self.lastFailure,
            restart: restart ?? self.restart, restartTotal: restartTotal,
            blocker: blocker ?? self.blocker, endpoints: endpoints,
            leftoverProcesses: leftoverProcesses, logsUnavailable: logsUnavailable,
            ownership: ownership, initialized: initialized ?? self.initialized,
            capabilities: capabilities, lastExit: lastExit, autostart: autostart, environments: nil)
    }
}

extension OperationSummary {
    /// The same operation on another instance.
    fileprivate func moved(to instance: UUID) -> OperationSummary {
        OperationSummary(
            id: id, kind: kind,
            resource: ResourceRef(kind: .instance, id: instance.uuidString.lowercased()),
            state: state, stage: stage, progress: progress, waitingOn: waitingOn,
            cancelRequestedAt: cancelRequestedAt, pointOfNoReturnAt: pointOfNoReturnAt,
            failure: failure, recovery: recovery, result: result, trigger: trigger,
            createdAt: createdAt, startedAt: startedAt, updatedAt: updatedAt, revision: revision,
            cancel: nil)
    }

    /// The same operation whose error carries no `operationId`.
    fileprivate var withoutOperationID: OperationSummary {
        let error = failure.map { info in
            OperationFailureInfo(
                reason: info.reason, retryable: info.retryable,
                error: ErrorBody(
                    code: info.error.code, category: info.error.category,
                    message: info.error.message, retryable: info.error.retryable,
                    resource: info.error.resource, fields: info.error.fields,
                    hint: info.error.hint, details: info.error.details, operationId: nil,
                    rule: info.error.rule, items: info.error.items))
        }
        return OperationSummary(
            id: id, kind: kind, resource: resource, state: state, stage: stage,
            progress: progress, waitingOn: waitingOn, cancelRequestedAt: cancelRequestedAt,
            pointOfNoReturnAt: pointOfNoReturnAt, failure: error, recovery: recovery,
            result: result, trigger: trigger, createdAt: createdAt, startedAt: startedAt,
            updatedAt: updatedAt, revision: revision, cancel: nil)
    }
}
