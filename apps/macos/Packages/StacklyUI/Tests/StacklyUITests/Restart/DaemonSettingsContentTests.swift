import StacklyClient
import Testing

@testable import StacklyUI

/// The Daemon row of Settings (spec 0030 AC-16): its status line and the reason the button
/// is off, for each connection the mockup shows.
struct DaemonSettingsContentTests {
    private func make(
        connected: Bool = true, protocolText: String? = "1.1",
        launch: LaunchContextKind? = .launchd,
        restarting: Bool = false
    ) -> DaemonSettingsContent {
        DaemonSettingsContent.make(
            isConnected: connected, protocolText: protocolText, launchContext: launch,
            isRestarting: restarting)
    }

    @Test func aConnectedLaunchdDaemonCanRestart() {
        let content = make()
        #expect(content.subtitle == "Connected over Unix socket · protocol 1.1")
        #expect(content.disabledReason == nil)
    }

    @Test func aDaemonWithoutAConnectionTurnsTheButtonOffWithTheReason() {
        let content = make(connected: false)
        #expect(content.subtitle == "Disconnected · reconnecting")
        #expect(content.disabledReason == "stacklyd is not connected.")
    }

    @Test func aDaemonOutsideLaunchdExplainsWhyItCannotRestart() {
        let content = make(launch: .manual)
        #expect(content.subtitle == "Runs outside launchd, so it can't restart itself")
        #expect(content.disabledReason == content.subtitle)
    }

    @Test func aRunningRestartTurnsTheButtonOff() {
        #expect(make(restarting: true).disabledReason == "A restart is already in progress.")
    }

    @Test func aDaemonThatDidNotSayHowItStartedIsLeftToTheDaemonToRefuse() {
        #expect(make(launch: nil).disabledReason == nil)
    }
}
