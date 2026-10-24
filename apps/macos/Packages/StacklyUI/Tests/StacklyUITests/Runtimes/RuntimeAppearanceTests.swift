import StacklyClient
import Testing

@testable import StacklyUI

/// The runtime status table of `apps/macos/design.md` (spec 0018 AC-17): the label and tone
/// of every state, and the status of a Catalog card.
struct RuntimeAppearanceTests {
    @Test(
        arguments: [
            (RuntimeState.installed, "Installed", Tone.positive),
            (.resolving, "Resolving…", .informative),
            (.downloading, "Downloading…", .informative),
            (.verifying, "Verifying…", .informative),
            (.installing, "Installing…", .informative),
            (.failed, "Failed", .critical),
            (.missing, "Missing", .critical),
            (.drifted, "Drifted", .caution),
            (.absent, "Removed", .neutral),
            (.unrecognized, "Unrecognized State", .caution),
        ])
    func everyStateHasItsLabelAndTone(state: RuntimeState, label: String, tone: Tone) {
        let appearance = RuntimeAppearance.appearance(of: state)
        #expect(appearance.label == label)
        #expect(appearance.tone == tone)
    }

    @Test func statesOfOneToneKeepTheirOwnSymbolSoTheyStayApartWithoutColor() {
        let informative: [RuntimeState] = [.resolving, .downloading, .verifying, .installing]
        let symbols = informative.map { RuntimeAppearance.appearance(of: $0).symbol }
        #expect(Set(symbols).count == symbols.count)
        #expect(
            RuntimeAppearance.appearance(of: .failed).symbol
                != RuntimeAppearance.appearance(of: .missing).symbol)
    }

    @Test func aStateFromANewerDaemonIsNeverGuessedIntoAnother() {
        let appearance = RuntimeAppearance.appearance(of: .unrecognized)
        #expect(appearance.detail?.text == "Update Stackly to see this state")
    }

    @Test func theCardStatusIsTheCountOfInstalledRuntimesInThePositiveTone() {
        let appearance = RuntimeAppearance.installedCount(3)
        #expect(appearance.label == "3 installed")
        #expect(appearance.tone == .positive)
    }

    @Test func aCardShowsInstalledOnlyForAnAvailableProviderWithRuntimes() {
        let nats = ProviderSummary.fixture(provider: "official/nats", displayName: "NATS")
        #expect(ProviderCard.status(of: nats, installedCount: 2).label == "2 installed")
        #expect(ProviderCard.status(of: nats, installedCount: 0).label == "Available")
        let planned = ProviderSummary.fixture(state: .planned, stateReason: .providerPlanned)
        #expect(ProviderCard.status(of: planned, installedCount: 1).label == "Planned")

    }
}
