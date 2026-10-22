import Foundation
import Testing

@testable import StacklyUI

/// The banner row for each agent state, the 15 second limit, and the hint (spec 0015,
/// *Banner per state*, AC-13).
struct AgentBannerTests {
    nonisolated static let now = FixtureClock.now
    nonisolated static let calendar = Calendar(identifier: .gregorian)

    nonisolated static func assessment(
        _ state: BackgroundAgentState, _ reason: UnavailableReason? = nil
    ) -> AgentAssessment {
        let recovery: RecoveryStep =
            switch state {
            case .enabled: .none
            case .requiresApproval: .approveInLoginItems
            case .disabled: .enableInLoginItems
            case .notRegistered: .register
            case .notFound: .moveOrReinstallApp
            case .unavailable: .checkAgain
            }
        return AgentAssessment(state: state, reason: reason, recovery: recovery)
    }

    nonisolated static func input(
        _ state: BackgroundAgentState, reason: UnavailableReason? = nil,
        since: Date = FixtureClock.now, lastKnownAt: Date? = nil,
        code: String? = "ipc.endpoint_missing", detail: String? = nil, isStarting: Bool = false
    ) -> AgentBanner.Input {
        AgentBanner.Input(
            status: .unreachable(lastKnownAt: lastKnownAt, since: since), failureCode: code,
            assessment: assessment(state, reason), detail: detail, isStarting: isStarting)
    }

    struct Row: CustomTestStringConvertible, Sendable {
        let name: String
        let input: AgentBanner.Input
        let tone: Tone
        let title: String
        let message: String
        let actions: [DaemonBannerAction]
        var testDescription: String { name }
    }

    nonisolated static let rows: [Row] = [
        Row(
            name: "not registered", input: input(.notRegistered), tone: .caution,
            title: "Stackly’s background service is off.",
            message:
                "Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it.",
            actions: [.turnOn, .openDiagnostics]),
        Row(
            name: "requires approval", input: input(.requiresApproval), tone: .caution,
            title: "Allow Stackly to run in the background.",
            message:
                "Turn on Stackly in System Settings > General > Login Items & Extensions. Stackly doesn’t change this setting for you.",
            actions: [.openLoginItems, .checkAgain]),
        Row(
            name: "disabled", input: input(.disabled), tone: .caution,
            title: "Stackly’s background service is turned off.",
            message:
                "Turn on Stackly in System Settings > General > Login Items & Extensions to run stacklyd again.",
            actions: [.openLoginItems, .checkAgain]),
        Row(
            name: "enabled under 15 seconds",
            input: input(.enabled, since: FixtureClock.secondsAgo(14)),
            tone: .informative, title: "stacklyd is starting…",
            message: "Actions are available once it’s ready.", actions: []),
        Row(
            name: "enabled at 15 seconds",
            input: input(.enabled, since: FixtureClock.secondsAgo(15)),
            tone: .caution, title: "stacklyd isn’t running.",
            message:
                "It was stopped or it exited. Start it again here, or run stackly daemon start in Terminal.",
            actions: [.startDaemon, .openDiagnostics]),
        Row(
            name: "enabled and starting",
            input: input(.enabled, since: FixtureClock.secondsAgo(60), isStarting: true),
            tone: .informative, title: "stacklyd is starting…",
            message: "Actions are available once it’s ready.", actions: []),
        Row(
            name: "not found", input: input(.notFound), tone: .critical,
            title: "Stackly can’t find its background service.",
            message:
                "macOS couldn’t find stacklyd for this copy of Stackly. Move Stackly to the Applications folder, or install it again.",
            actions: [.openDiagnostics]),
        Row(
            name: "helper missing", input: input(.unavailable, reason: .helperMissing),
            tone: .caution, title: "This build doesn’t include stacklyd.",
            message: "Start stacklyd yourself, or build the app with cargo xtask build-app.",
            actions: [.reconnect, .openDiagnostics]),
        Row(
            name: "not bundled", input: input(.unavailable, reason: .notBundled), tone: .caution,
            title: "This build doesn’t include stacklyd.",
            message: "Start stacklyd yourself, or build the app with cargo xtask build-app.",
            actions: [.reconnect, .openDiagnostics]),
        Row(
            name: "root override", input: input(.unavailable, reason: .rootOverride),
            tone: .caution, title: "STACKLY_ROOT is set.",
            message:
                "The background service only runs the default Stackly folder. Start stacklyd yourself for this folder.",
            actions: [.reconnect, .openDiagnostics]),
        Row(
            name: "status failed with a detail",
            input: input(.unavailable, reason: .statusFailed, detail: "timed out"),
            tone: .caution, title: "Stackly couldn’t check its background service.",
            message: "macOS didn’t answer (timed out). Check again in a moment.",
            actions: [.checkAgain, .openDiagnostics]),
        Row(
            name: "status unrecognized with a detail",
            input: input(.unavailable, reason: .statusUnrecognized, detail: "unknown status 7"),
            tone: .caution, title: "Stackly couldn’t check its background service.",
            message: "macOS didn’t answer (unknown status 7). Check again in a moment.",
            actions: [.checkAgain, .openDiagnostics]),
        Row(
            name: "unsupported os without a detail",
            input: input(.unavailable, reason: .unsupportedOs), tone: .caution,
            title: "Stackly couldn’t check its background service.",
            message: "macOS didn’t answer. Check again in a moment.",
            actions: [.checkAgain, .openDiagnostics]),
    ]

    private func message(_ banner: DaemonBannerContent) -> String {
        banner.message(
            now: Self.now, calendar: Self.calendar, locale: Locale(identifier: "en_US"),
            timeZone: TimeZone(identifier: "UTC") ?? .gmt)
    }

    @Test(arguments: rows)
    func everyRowOfTheTableIsBuiltFromItsInput(row: Row) throws {
        let banner = try #require(AgentBanner.content(row.input, now: Self.now))
        #expect(banner.tone == row.tone)
        #expect(banner.title == row.title)
        #expect(message(banner) == row.message)
        #expect(banner.actions == row.actions)
    }

    @Test func theLastKnownStateComesBeforeTheMessage() throws {
        let banner = try #require(
            AgentBanner.content(
                Self.input(.notRegistered, lastKnownAt: FixtureClock.minutesAgo(5)), now: Self.now))
        let text = message(banner)
        #expect(text.hasPrefix("Showing the last known state from "))
        #expect(
            text.hasSuffix(
                "Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it."
            ))
    }

    @Test func theRowStaysWhileTheStoresRefreshAfterTheLoss() throws {
        let unreachable = ShellStatus.unreachable(
            lastKnownAt: FixtureClock.minutesAgo(5), since: FixtureClock.now)
        var input = Self.input(.notRegistered)
        input.status = .refreshing(after: unreachable)
        let banner = try #require(AgentBanner.content(input, now: Self.now))
        #expect(banner.title == "Stackly’s background service is off.")
        input.status = .refreshing(after: .refreshing(after: unreachable))
        #expect(AgentBanner.content(input, now: Self.now)?.title == banner.title)
    }

    @Test func otherCodesStatusesAndNoAssessmentGiveNoRow() {
        var input = Self.input(.notRegistered)
        input.failureCode = "ipc.io_error"
        #expect(AgentBanner.content(input, now: Self.now) == nil)
        input.failureCode = nil
        #expect(AgentBanner.content(input, now: Self.now) == nil)
        input = Self.input(.notRegistered)
        input.assessment = nil
        #expect(AgentBanner.content(input, now: Self.now) == nil)
        input = Self.input(.notRegistered)
        input.status = .connected(.ready)
        #expect(AgentBanner.content(input, now: Self.now) == nil)
        input.status = .refreshing(after: .connecting(slow: false, since: Self.now))
        #expect(AgentBanner.content(input, now: Self.now) == nil)
    }

    @Test func bothCodesGiveTheSameRow() {
        let refused = AgentBanner.content(
            Self.input(.disabled, code: "ipc.connection_refused"), now: Self.now)
        #expect(refused?.title == "Stackly’s background service is turned off.")
    }

    @Test func theEnabledRowChangesAtTheFifteenthSecond() {
        let missing = Self.input(.enabled, since: FixtureClock.secondsAgo(14))
        #expect(AgentBanner.content(missing, now: Self.now)?.title == "stacklyd is starting…")
        #expect(
            AgentBanner.content(missing, now: Self.now.addingTimeInterval(1))?.title
                == "stacklyd isn’t running.")
    }

    @Test func theScheduleTicksOnlyWhileTheEnabledRowCountsToFifteen() {
        let fresh = Self.input(.enabled, since: FixtureClock.secondsAgo(14))
        #expect(DaemonBanner.ticksEverySecond(fresh, now: Self.now))
        #expect(
            DaemonBanner.ticksEverySecond(
                Self.input(.enabled, since: FixtureClock.secondsAgo(15)), now: Self.now) == false)
        #expect(
            DaemonBanner.ticksEverySecond(
                Self.input(.enabled, since: FixtureClock.secondsAgo(5), isStarting: true),
                now: Self.now) == false)
        #expect(DaemonBanner.ticksEverySecond(nil, now: Self.now) == false)
        #expect(
            DaemonBanner.ticksEverySecond(
                Self.input(.notRegistered, since: FixtureClock.secondsAgo(1)), now: Self.now)
                == false)
        #expect(
            DaemonBanner.ticksEverySecond(
                Self.input(.enabled, since: FixtureClock.secondsAgo(1), code: "ipc.io_error"),
                now: Self.now) == false)
    }

    @Test func theHintIsTheTitleAndTheMessageWithoutTheLastKnownNote() throws {
        let input = Self.input(.notRegistered, lastKnownAt: FixtureClock.minutesAgo(5))
        let hint = try #require(AgentBanner.hint(input, now: Self.now))
        #expect(
            hint
                == "Stackly’s background service is off. Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it."
        )
        var other = input
        other.failureCode = "ipc.io_error"
        #expect(AgentBanner.hint(other, now: Self.now) == nil)
    }

    @Test func theActionsHaveTheirTitles() {
        #expect(DaemonBannerAction.turnOn.title == "Turn On")
        #expect(DaemonBannerAction.openLoginItems.title == "Open Login Items")
        #expect(DaemonBannerAction.checkAgain.title == "Check Again")
        #expect(DaemonBannerAction.startDaemon.title == "Start stacklyd")
    }
}
