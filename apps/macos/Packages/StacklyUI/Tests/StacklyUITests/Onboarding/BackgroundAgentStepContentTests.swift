import Foundation
import Testing

@testable import StacklyUI

/// Every row of the *Kartu Background Agent* table, with the clock injected (spec 0016, AC-5).
struct BackgroundAgentStepContentTests {
    nonisolated static let since = Date(timeIntervalSince1970: 1_792_300_000)
    static let footer = StatusAppearance(
        label: "stacklyd unavailable", tone: .critical, symbol: Tone.critical.symbol)

    static func assessment(
        _ state: BackgroundAgentState, reason: UnavailableReason? = nil
    ) -> AgentAssessment {
        AgentAssessment(state: state, reason: reason, recovery: .none)
    }

    static func input(
        _ state: BackgroundAgentState?, status: ShellStatus, code: String? = nil,
        reason: UnavailableReason? = nil, detail: String? = nil, isStarting: Bool = false
    ) -> AgentBanner.Input {
        AgentBanner.Input(
            status: status, failureCode: code,
            assessment: state.map { assessment($0, reason: reason) }, detail: detail,
            isStarting: isStarting)
    }

    static func missing(since: Date = since) -> ShellStatus {
        .unreachable(lastKnownAt: nil, since: since)
    }

    static func make(
        _ input: AgentBanner.Input, after seconds: TimeInterval = 0,
        lastError: AgentActionError? = nil, isWorking: Bool = false,
        footer: StatusAppearance = footer
    ) -> BackgroundAgentStepContent {
        BackgroundAgentStepContent.make(
            input, footer: footer, now: since.addingTimeInterval(seconds), lastError: lastError,
            isWorking: isWorking)
    }

    // MARK: - assessment nil and the tone rows

    @Test func withoutAnAssessmentTheCardChecks() {
        let content = Self.make(Self.input(nil, status: .connected(.ready)))
        #expect(content.icon == .progress)
        #expect(content.title == "Checking the background service…")
        #expect(content.detail == nil)
        #expect(content.action == nil)
        #expect(content.status == nil)
    }

    @Test func notRegisteredOffersTurnOn() {
        let content = Self.make(
            Self.input(.notRegistered, status: Self.missing(), code: "ipc.endpoint_missing"))
        #expect(content.title == "Background service is off")
        #expect(
            content.detail
                == "Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it."
        )
        #expect(content.action == .turnOn)
        #expect(content.icon == .symbol(Tone.caution.symbol, .caution))
        #expect(content.status == nil)
    }

    @Test func requiresApprovalOffersLoginItemsWithAClock() {
        let content = Self.make(Self.input(.requiresApproval, status: .connected(.ready)))
        #expect(content.title == "Waiting for approval")
        #expect(
            content.detail
                == "Turn on Stackly in System Settings > General > Login Items & Extensions. stacklyd can’t run until you do."
        )
        #expect(content.action == .openLoginItems)
        #expect(content.action?.title == "Open System Settings…")
        #expect(content.icon == .symbol("clock", .caution))
    }

    @Test func disabledOffersLoginItems() {
        let content = Self.make(Self.input(.disabled, status: .connected(.ready)))
        #expect(content.title == "Turned off in Login Items")
        #expect(
            content.detail
                == "Turn on Stackly in System Settings > General > Login Items & Extensions to run stacklyd again."
        )
        #expect(content.action == .openLoginItems)
    }

    @Test func notFoundIsCriticalWithoutAnAction() {
        let content = Self.make(Self.input(.notFound, status: .connected(.ready)))
        #expect(content.title == "Stackly can’t find its background service")
        #expect(
            content.detail
                == "macOS couldn’t find stacklyd for this copy of Stackly. Move Stackly to the Applications folder, or install it again."
        )
        #expect(content.action == nil)
        #expect(content.icon == .symbol(Tone.critical.symbol, .critical))
    }

    // MARK: - enabled

    @Test(arguments: [
        ShellStatus.connected(.ready), .refreshing(after: .connected(.ready)),
        .starting(since: since),
    ])
    func enabledAndConnectedIsAllowedWithTheDaemonStatus(status: ShellStatus) {
        let content = Self.make(Self.input(.enabled, status: status))
        #expect(content.title == "Allowed in Login Items")
        #expect(content.detail == "stacklyd starts when you log in.")
        #expect(content.status == Self.footer)
        #expect(content.action == nil)
        #expect(content.icon == .symbol(Tone.positive.symbol, .positive))
    }

    @Test(arguments: ["ipc.endpoint_missing", "ipc.connection_refused"])
    func enabledWithoutADaemonIsStartingForFifteenSeconds(code: String) {
        let input = Self.input(.enabled, status: Self.missing(), code: code)
        for seconds in [0.0, 14.0, 14.9] {
            let content = Self.make(input, after: seconds)
            #expect(content.title == "Allowed in Login Items", "\(seconds)")
            #expect(content.detail == "stacklyd is starting…", "\(seconds)")
            #expect(content.action == nil, "\(seconds)")
            #expect(content.status == Self.footer, "the footer stays unavailable")
            #expect(content.icon == .symbol(Tone.informative.symbol, .informative))
        }
    }

    @Test(arguments: ["ipc.endpoint_missing", "ipc.connection_refused"])
    func enabledWithoutADaemonOffersStartAtFifteenSeconds(code: String) {
        let input = Self.input(.enabled, status: Self.missing(), code: code)
        for seconds in [15.0, 16.0, 600.0] {
            let content = Self.make(input, after: seconds)
            #expect(content.title == "Allowed, but stacklyd isn’t running", "\(seconds)")
            #expect(
                content.detail
                    == "It was stopped or it exited. Start it again here, or run stackly daemon start in Terminal."
            )
            #expect(content.action == .start)
            #expect(content.action?.title == "Start stacklyd")
            #expect(content.icon == .symbol(Tone.caution.symbol, .caution))
        }
    }

    @Test func whileTheDaemonIsWaitedForStartIsNeverOffered() {
        let input = Self.input(
            .enabled, status: Self.missing(), code: "ipc.endpoint_missing", isStarting: true)
        let content = Self.make(input, after: 120)
        #expect(content.detail == "stacklyd is starting…")
        #expect(content.action == nil)
    }

    @Test(arguments: ["ipc.call_timeout", "ipc.io_error"])
    func anotherConnectionCodeIsNeverOfferedStart(code: String) {
        let content = Self.make(
            Self.input(.enabled, status: Self.missing(), code: code), after: 120)
        #expect(content.action == nil)
        #expect(
            content.detail
                == "Stackly can’t reach stacklyd right now. Diagnostics has the details after setup."
        )
    }

    @Test func anUnreachableDaemonWithoutACodeIsOtherToo() {
        let content = Self.make(Self.input(.enabled, status: Self.missing()), after: 120)
        #expect(content.action == nil)
        #expect(content.detail?.hasPrefix("Stackly can’t reach stacklyd") == true)
    }

    @Test func enabledWhileConnectingSaysConnecting() {
        let content = Self.make(
            Self.input(.enabled, status: .connecting(slow: true, since: Self.since)))
        #expect(content.title == "Allowed in Login Items")
        #expect(content.detail == "Connecting to stacklyd…")
        #expect(content.action == nil)
        #expect(content.icon == .symbol(Tone.informative.symbol, .informative))
    }

    @Test(arguments: [
        ShellStatus.stopping(since: since),
        .incompatible(.handshakeRejected(code: "request.x"), since: since),
        .untrusted(.peerRejected, since: since),
        .recovery(since: since),
        .misconfigured(.rootTooLong, since: since),
    ])
    func enabledInAnyOtherPresenceTakesTheFooterTone(status: ShellStatus) {
        let content = Self.make(Self.input(.enabled, status: status), after: 120)
        #expect(content.title == "Allowed in Login Items")
        #expect(content.action == nil)
        #expect(content.icon == .symbol(Tone.critical.symbol, .critical))
        #expect(content.status == Self.footer)
    }

    @Test func aRefreshingStatusReadsAsConnectedWhateverCameBefore() {
        let status = ShellStatus.refreshing(after: Self.missing())
        let input = Self.input(.enabled, status: status, code: "ipc.endpoint_missing")
        let content = Self.make(input, after: 120)
        #expect(content.detail == "stacklyd starts when you log in.")
    }

    // MARK: - unavailable

    @Test(arguments: [UnavailableReason.helperMissing, .notBundled])
    func aBuildWithoutStacklydHasNoAction(reason: UnavailableReason) {
        let content = Self.make(
            Self.input(.unavailable, status: .connected(.ready), reason: reason))
        #expect(content.title == "This build doesn’t include stacklyd")
        #expect(
            content.detail
                == "Start stacklyd yourself, or build the app with cargo xtask build-app.")
        #expect(content.action == nil)
    }

    @Test func aRootOverrideHasNoAction() {
        let content = Self.make(
            Self.input(.unavailable, status: .connected(.ready), reason: .rootOverride))
        #expect(content.title == "STACKLY_ROOT is set")
        #expect(
            content.detail
                == "The background service only runs the default Stackly folder. Start stacklyd yourself for this folder."
        )
        #expect(content.action == nil)
    }

    @Test(arguments: [UnavailableReason.statusFailed, .statusUnrecognized, .unsupportedOs, nil])
    func anUnreadableStatusOffersCheckAgain(reason: UnavailableReason?) {
        let withDetail = Self.make(
            Self.input(
                .unavailable, status: .connected(.ready), reason: reason, detail: "timed out"))
        #expect(withDetail.title == "Stackly couldn’t check its background service")
        #expect(
            withDetail.detail == "macOS didn’t answer (timed out). Check again in a moment.")
        #expect(withDetail.action == .checkAgain)
        #expect(withDetail.action?.title == "Check Again")
        let without = Self.make(
            Self.input(.unavailable, status: .connected(.ready), reason: reason))
        #expect(without.detail == "macOS didn’t answer. Check again in a moment.")
    }

    // MARK: - working and the last error

    @Test func workingIsCarriedSoTheViewDisablesTheAction() {
        let input = Self.input(.notRegistered, status: Self.missing(), code: "ipc.endpoint_missing")
        let content = Self.make(input, isWorking: true)
        #expect(content.isWorking)
        #expect(content.action == .turnOn)
        #expect(OnboardingCopy.working == "Working…")
    }

    @Test func theLastErrorShowsUnderTheCardWithTheTextOfTheError() {
        let input = Self.input(.notRegistered, status: Self.missing(), code: "ipc.endpoint_missing")
        let content = Self.make(input, lastError: .daemonDidNotStart(seconds: 30))
        #expect(content.lastError == "stacklyd didn’t start within 30 seconds.")
    }

    @Test func theLastErrorStaysOutOfAnEnabledConnectedRow() {
        let content = Self.make(
            Self.input(.enabled, status: .connected(.ready)),
            lastError: .daemonDidNotStart(seconds: 30))
        #expect(content.lastError == nil)
    }

    @Test func theLastErrorShowsInAnEnabledRowWhoseDaemonIsMissing() {
        let input = Self.input(.enabled, status: Self.missing(), code: "ipc.endpoint_missing")
        let content = Self.make(input, after: 120, lastError: .registrationNotKept)
        #expect(content.lastError == "macOS didn’t keep the registration.")
    }

    // MARK: - rows are told apart

    @Test func twoContentsOfTheSameRowDifferOnlyInWorkingAndError() {
        let input = Self.input(.notRegistered, status: Self.missing(), code: "ipc.endpoint_missing")
        let a = Self.make(input)
        let b = Self.make(input, lastError: .registrationNotKept, isWorking: true)
        #expect(BackgroundAgentStepContent.isSameRow(a, b))
        let c = Self.make(Self.input(.disabled, status: Self.missing()))
        #expect(BackgroundAgentStepContent.isSameRow(a, c) == false)
    }
}
