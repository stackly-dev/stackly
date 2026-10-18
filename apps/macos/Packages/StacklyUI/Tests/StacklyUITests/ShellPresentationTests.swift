import Foundation
import Testing

@testable import StacklyUI

/// The daemon status table of spec 0013 ("Tampilan status daemon"): footer, banner,
/// actions, mutation gate, and the page placeholder for every row (AC-9, AC-10, AC-11).
struct ShellPresentationTests {
    nonisolated static let since = Date(timeIntervalSince1970: 1_792_245_720)
    nonisolated static let unavailable = "Actions are disabled until the daemon is reachable."
    nonisolated static let incompatible = "This version of Stackly can’t talk to stacklyd."

    /// One row of the table and what it must show.
    nonisolated struct Expected: CustomTestStringConvertible, Sendable {
        let name: String
        let status: ShellStatus
        let footer: String
        let footerTone: Tone
        let banner: (tone: Tone, title: String, message: String)?
        let actions: [DaemonBannerAction]
        /// The blocked reason, or `nil` when mutations are available.
        let gate: String?
        let pageWaits: Bool

        var testDescription: String { name }
    }

    nonisolated static let rows: [Expected] = [
        Expected(
            name: "connected, ready", status: .connected(.ready),
            footer: "stacklyd running", footerTone: .positive, banner: nil, actions: [],
            gate: nil, pageWaits: false),
        Expected(
            name: "connected, degraded", status: .connected(.degraded),
            footer: "stacklyd degraded", footerTone: .caution, banner: nil, actions: [],
            gate: nil, pageWaits: false),
        Expected(
            name: "connected, unknown health", status: .connected(.unknown),
            footer: "stacklyd status unknown", footerTone: .caution, banner: nil, actions: [],
            gate: nil, pageWaits: false),
        Expected(
            name: "starting", status: .starting(since: since),
            footer: "stacklyd starting…", footerTone: .caution,
            banner: (
                .informative, "stacklyd is starting.", "Actions are available once it’s ready."
            ),
            actions: [], gate: "stacklyd is starting.", pageWaits: false),
        Expected(
            name: "stopping", status: .stopping(since: since),
            footer: "stacklyd stopping…", footerTone: .caution,
            banner: (
                .informative, "stacklyd is shutting down.", "Stackly reconnects when it’s back."
            ),
            actions: [.openDiagnostics], gate: "stacklyd is shutting down.", pageWaits: true),
        Expected(
            name: "connecting", status: .connecting(slow: false, since: since),
            footer: "Connecting…", footerTone: .caution, banner: nil, actions: [],
            gate: "Connecting to stacklyd…", pageWaits: false),
        Expected(
            name: "connecting, slow", status: .connecting(slow: true, since: since),
            footer: "Connecting…", footerTone: .caution,
            banner: (
                .informative, "Connecting to stacklyd…",
                "Actions are available once it’s connected."
            ),
            actions: [.openDiagnostics], gate: "Connecting to stacklyd…", pageWaits: false),
        Expected(
            name: "unreachable, no state yet", status: .unreachable(lastKnownAt: nil, since: since),
            footer: "stacklyd unavailable", footerTone: .critical,
            banner: (.caution, "Can’t connect to stacklyd.", unavailable),
            actions: [.reconnect, .openDiagnostics], gate: "stacklyd is unavailable.",
            pageWaits: true),
        Expected(
            name: "unreachable, last known state",
            status: .unreachable(lastKnownAt: since.addingTimeInterval(-120), since: since),
            footer: "stacklyd unavailable", footerTone: .critical,
            banner: (
                .caution, "Can’t connect to stacklyd.",
                "Showing the last known state from 2:00\u{202F}PM. " + unavailable
            ),
            actions: [.reconnect, .openDiagnostics], gate: "stacklyd is unavailable.",
            pageWaits: true),
        Expected(
            name: "incompatible, major mismatch",
            status: .incompatible(
                .majorMismatch(daemonProtocol: "2.0", appProtocol: "1.0"), since: since),
            footer: "stacklyd incompatible", footerTone: .critical,
            banner: (
                .critical, incompatible,
                "stacklyd speaks protocol 2.0; this app speaks 1.0. Install matching versions of Stackly and stacklyd."
            ),
            actions: [.reconnect, .openDiagnostics], gate: incompatible, pageWaits: true),
        Expected(
            name: "incompatible, handshake rejected",
            status: .incompatible(.handshakeRejected(code: "request.invalid_params"), since: since),
            footer: "stacklyd incompatible", footerTone: .critical,
            banner: (
                .critical, incompatible,
                "stacklyd rejected the handshake (request.invalid_params). Install matching versions of Stackly and stacklyd."
            ),
            actions: [.reconnect, .openDiagnostics], gate: incompatible, pageWaits: true),
        Expected(
            name: "untrusted", status: .untrusted(.endpoint(reason: "symlink"), since: since),
            footer: "stacklyd untrusted", footerTone: .critical,
            banner: (
                .critical, "Stackly didn’t connect to stacklyd.",
                "The socket failed a security check: the path is a symbolic link. Nothing was sent."
            ),
            actions: [.reconnect, .openDiagnostics],
            gate: "stacklyd’s socket failed a security check.", pageWaits: true),
        Expected(
            name: "recovery", status: .recovery(since: since),
            footer: "stacklyd in recovery", footerTone: .critical,
            banner: (
                .critical, "Stackly’s registry needs recovery.",
                "stacklyd started in recovery mode and didn’t change your data. Instances can’t be managed until the registry is repaired."
            ),
            actions: [.openDiagnostics], gate: "Stackly’s registry needs recovery.",
            pageWaits: true),
        Expected(
            name: "misconfigured", status: .misconfigured(.rootEmpty, since: since),
            footer: "Stackly misconfigured", footerTone: .critical,
            banner: (
                .critical, "STACKLY_ROOT is invalid.",
                "STACKLY_ROOT is set but empty. Fix the variable and relaunch Stackly."
            ),
            actions: [.openDiagnostics], gate: "STACKLY_ROOT is invalid.", pageWaits: true),
    ]

    @Test(arguments: rows)
    func everyStatusRowShowsItsFooterBannerAndGate(row: Expected) {
        let presentation = ShellPresentation(status: row.status, lastErrorCode: "ipc.io_error")

        #expect(presentation.footer.label == row.footer)
        #expect(presentation.footer.tone == row.footerTone)
        #expect(presentation.footer.symbol == row.footerTone.symbol)
        #expect(presentation.footer.detail == nil)
        if let expected = row.banner {
            let banner = presentation.banner
            #expect(banner?.tone == expected.tone)
            #expect(banner?.title == expected.title)
            #expect(banner.map(Self.message) == expected.message)
            #expect(banner?.actions == row.actions)
        } else {
            #expect(presentation.banner == nil)
        }
        #expect(presentation.mutationAvailability.blockedReason == row.gate)
        if row.gate != nil {
            #expect(
                presentation.mutationAvailability
                    == .blocked(reason: row.gate ?? "", since: Self.since))
        }
        #expect((presentation.pageFailure != nil) == row.pageWaits)
    }

    @Test func pagesWaitWithTheLastErrorCode() throws {
        let presentation = ShellPresentation(
            status: .unreachable(lastKnownAt: nil, since: Self.since),
            lastErrorCode: "ipc.endpoint_missing"
        )
        let failure = try #require(presentation.pageFailure)

        #expect(failure.title == "Waiting for stacklyd")
        #expect(failure.message == "Stackly shows this page once the daemon is reachable.")
        #expect(failure.code == "ipc.endpoint_missing")
    }

    @Test func refreshingAfterLaunchKeepsTheBannerAndSaysTheStateIsLoading() {
        let previous = ShellStatus.connecting(slow: true, since: Self.since)
        let presentation = ShellPresentation(status: .refreshing(after: previous))

        #expect(presentation.footer.label == "Refreshing…")
        #expect(presentation.footer.tone == .caution)
        #expect(presentation.banner == ShellPresentation(status: previous).banner)
        #expect(
            presentation.mutationAvailability
                == .blocked(reason: "Loading the latest state.", since: Self.since)
        )
        #expect(presentation.pageFailure == nil)
    }

    @Test func refreshingAfterAnOutageKeepsTheOutageBannerAndGate() {
        let previous = ShellStatus.unreachable(
            lastKnownAt: Self.since.addingTimeInterval(-60),
            since: Self.since
        )
        let presentation = ShellPresentation(status: .refreshing(after: previous))
        let before = ShellPresentation(status: previous)

        #expect(presentation.footer.label == "Refreshing…")
        #expect(presentation.banner == before.banner)
        #expect(presentation.mutationAvailability == before.mutationAvailability)
        #expect(presentation.pageFailure == nil)
        #expect(presentation.isConnected == false)
        #expect(presentation.hasConnection)
    }

    @Test func onlyAConnectedDaemonMakesMutationsAvailable() {
        for row in Self.rows {
            let presentation = ShellPresentation(status: row.status)
            let connected: Bool =
                if case .connected = row.status { true } else { false }
            #expect((presentation.mutationAvailability == .available) == connected, "\(row.name)")
            #expect(presentation.isConnected == connected, "\(row.name)")
        }
    }

    @Test func noBannerActionChangesData() {
        for row in Self.rows {
            for action in ShellPresentation(status: row.status).banner?.actions ?? [] {
                #expect(action == .reconnect || action == .openDiagnostics)
            }
        }
        #expect(DaemonBannerAction.reconnect.title == "Reconnect")
        #expect(DaemonBannerAction.openDiagnostics.title == "Open Diagnostics")
    }

    nonisolated static let misconfigurations: [(ShellStatus.Misconfiguration, String, String)] = [
        (
            .rootEmpty, "STACKLY_ROOT is invalid.",
            "STACKLY_ROOT is set but empty. Fix the variable and relaunch Stackly."
        ),
        (
            .rootNotAbsolute, "STACKLY_ROOT is invalid.",
            "STACKLY_ROOT must be an absolute path. Fix the variable and relaunch Stackly."
        ),
        (
            .rootMissing, "STACKLY_ROOT is invalid.",
            "The folder in STACKLY_ROOT doesn’t exist. Fix the variable and relaunch Stackly."
        ),
        (
            .rootUnreadable, "STACKLY_ROOT is invalid.",
            "Stackly couldn’t read the folder in STACKLY_ROOT. Fix the variable and relaunch Stackly."
        ),
        (
            .rootTooLong, "STACKLY_ROOT is invalid.",
            "The socket path for STACKLY_ROOT would be too long. Use a shorter folder and relaunch Stackly."
        ),
        (
            .tempDirUnavailable, "Stackly can’t find its socket folder.",
            "macOS didn’t report a temporary folder for your user. Restart your Mac and try again."
        ),
        (
            .invalidClientVersion, "This build of Stackly is invalid.",
            "Its version number can’t be sent to stacklyd. Install a release build."
        ),
    ]

    @Test func everyMisconfigurationHasItsTitleAndMessage() {
        #expect(Self.misconfigurations.count == ShellStatus.Misconfiguration.allCases.count)
        for (problem, title, message) in Self.misconfigurations {
            let presentation = ShellPresentation(status: .misconfigured(problem, since: Self.since))
            #expect(presentation.banner?.title == title)
            #expect(presentation.banner.map(Self.message) == message)
            #expect(presentation.mutationAvailability.blockedReason == title)
            #expect(presentation.isMisconfigured)
        }
    }

    /// The banner message in en_US and UTC, on the day of ``since``.
    nonisolated static func message(_ banner: DaemonBannerContent) -> String {
        banner.message(
            now: since,
            calendar: Calendar(identifier: .gregorian),
            locale: Locale(identifier: "en_US"),
            timeZone: .gmt
        )
    }
}
