import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The words the shell hands to VoiceOver and to the keyboard: sidebar rows,
/// badges, the footer, banner buttons, and the two announcements (spec 0013
/// AC-21). Using the shell with the keyboard and VoiceOver on stays a manual
/// check (record cek 6).
struct ShellAccessibilityTests {
    nonisolated static let since = Date(timeIntervalSince1970: 1_792_245_720)

    /// One status per row of the daemon status table, with both variants where
    /// the copy differs.
    nonisolated static let statuses: [ShellStatus] =
        [
            .connected(.ready), .connected(.degraded), .connected(.unknown),
            .refreshing(after: .unreachable(lastKnownAt: since, since: since)),
            .starting(since: since), .stopping(since: since),
            .connecting(slow: false, since: since), .connecting(slow: true, since: since),
            .unreachable(lastKnownAt: nil, since: since),
            .unreachable(lastKnownAt: since, since: since),
            .incompatible(.majorMismatch(daemonProtocol: "2.0", appProtocol: "1.1"), since: since),
            .incompatible(.handshakeRejected(code: "request.invalid_params"), since: since),
            .untrusted(.endpoint(reason: "symlink"), since: since),
            .untrusted(.peerRejected, since: since),
            .recovery(since: since),
        ] + ShellStatus.Misconfiguration.allCases.map { .misconfigured($0, since: since) }

    @Test(
        .evidence(
            covers: ["REQ-027", "NFR-012"],
            expected:
                "every sidebar row, badge, footer status, and banner button has the words VoiceOver and the keyboard use; the live keyboard and VoiceOver run is a manual check"
        ))
    func everyElementHasTheWordsVoiceOverReads() {
        let ids = SidebarSection.allCases.map { AccessibilityID.Sidebar.row($0) }
        #expect(Set(ids).count == SidebarSection.allCases.count)
        for section in SidebarSection.allCases {
            #expect(section.title.isEmpty == false)
        }

        let counts = SidebarCounts(instances: 2, runningOperations: 1)
        #expect(
            SidebarSection.instances.badge(counts: counts, isConnected: true)?.spokenValue
                == "2 instances")
        #expect(
            SidebarSection.operations.badge(counts: counts, isConnected: true)?.spokenValue
                == "1 running")

        for status in Self.statuses {
            let presentation = ShellPresentation(status: status)
            #expect(presentation.footer.label.isEmpty == false, "\(status)")
            guard let banner = presentation.banner else { continue }
            #expect(banner.title.isEmpty == false, "\(status)")
            let message = banner.message(
                now: Self.since, calendar: Calendar(identifier: .gregorian),
                locale: Locale(identifier: "en_US"), timeZone: .gmt)
            #expect(message.isEmpty == false, "\(status)")
            #expect(Set(banner.actions).count == banner.actions.count, "\(status)")
            for action in banner.actions {
                #expect(action.title.isEmpty == false)
            }
        }
        #expect(DaemonBannerAction.reconnect.title == "Reconnect")
        #expect(DaemonBannerAction.openDiagnostics.title == "Open Diagnostics")
    }

    @Test func theTwoAnnouncementsUseTheSpecsWords() {
        #expect(DaemonModel.Announcement.unreachable == "Can’t connect to stacklyd.")
        #expect(DaemonModel.Announcement.reconnected == "Reconnected. State refreshed.")
    }

    @Test func aBannerThatCanReconnectAlsoOffersDiagnosticsAndNeverTheOtherWayAround() {
        for status in Self.statuses {
            guard let banner = ShellPresentation(status: status).banner else { continue }
            if banner.actions.contains(.reconnect) {
                #expect(banner.actions.contains(.openDiagnostics), "\(status)")
            }
        }
    }
}
