import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The `not_found` announcement of the onboarding and its pair with `DaemonModel` (spec 0016,
/// AC-10): at every presence exactly one of the two speaks.
@Suite(.timeLimit(.minutes(1)))
struct OnboardingAnnouncementsTests {
    static let title = "Stackly can’t find its background service."
    static let since = BackgroundAgentStepContentTests.since

    // MARK: - The pure decision

    /// The banner input for `status` and `code`, with the agent at `state`.
    private static func input(
        _ status: ShellStatus, code: String? = nil, state: BackgroundAgentState = .notFound
    ) -> AgentBanner.Input {
        BackgroundAgentStepContentTests.input(state, status: status, code: code)
    }

    private static func said(
        old: BackgroundAgentState?, new: BackgroundAgentState?, input: AgentBanner.Input?
    ) -> String? {
        OnboardingAnnouncements.agentNotFound(old: old, new: new, input: input, now: since)
    }

    @Test(arguments: ["ipc.endpoint_missing", "ipc.connection_refused"])
    func aDaemonThatIsMissingGivesTheBannerTitle(code: String) {
        let input = Self.input(.unreachable(lastKnownAt: nil, since: Self.since), code: code)
        #expect(Self.said(old: .notRegistered, new: .notFound, input: input) == Self.title)
        let refreshing = Self.input(
            .refreshing(after: .unreachable(lastKnownAt: nil, since: Self.since)), code: code)
        #expect(Self.said(old: .notRegistered, new: .notFound, input: refreshing) == Self.title)
    }

    @Test func presencesWithoutAnAgentRowStayQuiet() {
        let statuses: [(ShellStatus, String?)] = [
            (.connected(.ready), nil),
            (.connecting(slow: false, since: Self.since), nil),
            (.stopping(since: Self.since), nil),
            (.incompatible(.handshakeRejected(code: "request.x"), since: Self.since), nil),
            (.unreachable(lastKnownAt: nil, since: Self.since), "ipc.call_timeout"),
            (.refreshing(after: .connected(.ready)), nil),
            (
                .refreshing(after: .unreachable(lastKnownAt: nil, since: Self.since)),
                "ipc.call_timeout"
            ),
        ]
        for (status, code) in statuses {
            let input = Self.input(status, code: code)
            #expect(
                Self.said(old: .notRegistered, new: .notFound, input: input) == nil,
                "\(status)")
        }
    }

    @Test(arguments: [
        BackgroundAgentState.notRegistered, .requiresApproval, .disabled, .enabled,
    ])
    func aChangeFromAnotherStateToNotFoundIsSaid(old: BackgroundAgentState) {
        let input = Self.input(
            .unreachable(lastKnownAt: nil, since: Self.since), code: "ipc.endpoint_missing")
        #expect(Self.said(old: old, new: .notFound, input: input) == Self.title)
    }

    @Test func theFirstStateAfterLaunchIsNeverSaid() {
        let input = Self.input(
            .unreachable(lastKnownAt: nil, since: Self.since), code: "ipc.endpoint_missing")
        #expect(Self.said(old: nil, new: .notFound, input: input) == nil)
    }

    @Test func notFoundToNotFoundIsNotAChange() {
        let input = Self.input(
            .unreachable(lastKnownAt: nil, since: Self.since), code: "ipc.endpoint_missing")
        #expect(Self.said(old: .notFound, new: .notFound, input: input) == nil)
    }

    @Test func leavingNotFoundAndOtherStatesAreNotSaid() {
        let input = Self.input(
            .unreachable(lastKnownAt: nil, since: Self.since), code: "ipc.endpoint_missing",
            state: .notRegistered)
        #expect(Self.said(old: .notFound, new: .notRegistered, input: input) == nil)
        #expect(Self.said(old: .notRegistered, new: .requiresApproval, input: input) == nil)
    }

    @Test func withoutAnInputNothingIsSaid() {
        #expect(Self.said(old: .notRegistered, new: .notFound, input: nil) == nil)
    }

    // MARK: - The pair with DaemonModel

    private static func body(_ code: String) -> ErrorBody {
        ErrorBody(
            code: code, category: .unavailable, message: "m", retryable: true, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
    }

    private static func unreachable(_ code: String) -> ConnectionState {
        let launch = DaemonModelTests.launch
        return .unreachable(
            body(code), since: launch.addingTimeInterval(5),
            nextAttempt: launch.addingTimeInterval(5.5))
    }

    /// How a presence is reached.
    enum Setup: CaseIterable, CustomTestStringConvertible {
        case connected, connecting, incompatible
        case missing, refused, timeout
        case refreshingAfterMissing, refreshingAfterRefused, refreshingAfterConnected,
            refreshingAfterTimeout

        var testDescription: String { "\(self)" }

        func reach(_ rig: DaemonModelAgentTests.Rig) async throws {
            switch self {
            case .connected:
                try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
                await rig.send(.caughtUp)
                try await waitUntil { rig.model.presence == .connected(health: .ready) }
            case .connecting:
                try await rig.push(.connecting(attempt: 1))
            case .incompatible:
                try await rig.push(
                    .incompatible(
                        .handshakeRejected(OnboardingAnnouncementsTests.body("request.x")),
                        since: DaemonModelTests.launch, nextAttempt: DaemonModelTests.launch))
            case .missing:
                try await rig.push(OnboardingAnnouncementsTests.unreachable("ipc.endpoint_missing"))
            case .refused:
                try await rig.push(
                    OnboardingAnnouncementsTests.unreachable("ipc.connection_refused"))
            case .timeout:
                try await rig.push(OnboardingAnnouncementsTests.unreachable("ipc.call_timeout"))
            case .refreshingAfterMissing:
                try await rig.push(OnboardingAnnouncementsTests.unreachable("ipc.endpoint_missing"))
                try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
            case .refreshingAfterRefused:
                try await rig.push(
                    OnboardingAnnouncementsTests.unreachable("ipc.connection_refused"))
                try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
            case .refreshingAfterTimeout:
                try await rig.push(OnboardingAnnouncementsTests.unreachable("ipc.call_timeout"))
                try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
            case .refreshingAfterConnected:
                try await rig.push(.connected(DaemonModelTests.daemon(.ready)))
            }
        }
    }

    @Test(arguments: Setup.allCases)
    func exactlyOneOfTheOnboardingAndTheModelSaysNotFound(setup: Setup) async throws {
        let rig = DaemonModelAgentTests.Rig(
            afterRegister: DaemonModelAgentTests.fresh(.notFound),
            initialProbe: DaemonModelAgentTests.fresh())
        try await setup.reach(rig)
        for _ in 0..<20 { await Task.yield() }
        let before = rig.announced.filter { $0 == Self.title }.count
        let old = rig.agent?.assessment?.state
        await rig.agent?.turnOn()
        #expect(rig.agent?.assessment?.state == .notFound)
        let onboarding = OnboardingAnnouncements.agentNotFound(
            old: old, new: rig.agent?.assessment?.state, input: rig.model.agentBannerInput,
            now: rig.clock)
        let model = rig.announced.filter { $0 == Self.title }.count - before
        #expect((onboarding == nil ? 0 : 1) + model == 1, "\(setup): \(onboarding ?? "-") \(model)")
        // The same state again is not a change for either.
        await rig.agent?.refresh()
        #expect(rig.announced.filter { $0 == Self.title }.count - before == model)
        #expect(
            OnboardingAnnouncements.agentNotFound(
                old: .notFound, new: .notFound, input: rig.model.agentBannerInput, now: rig.clock)
                == nil)
    }
}
