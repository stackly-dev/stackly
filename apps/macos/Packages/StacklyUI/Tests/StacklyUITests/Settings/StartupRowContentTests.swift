import Foundation
import Testing

@testable import StacklyUI

/// The Start stacklyd at login row for each of the six agent states (spec 0032 AC-15).
struct StartupRowContentTests {
    private func content(
        _ state: BackgroundAgentState, reason: UnavailableReason? = nil, count: Int = 2,
        isWorking: Bool = false
    ) -> StartupRowContent {
        StartupRowContent.make(
            assessment: AgentAssessment(state: state, reason: reason, recovery: .none),
            atLoginCount: count, isWorking: isWorking)
    }

    @Test func nothingIsKnownBeforeTheFirstAnswer() {
        let row = StartupRowContent.make(assessment: nil, atLoginCount: 3, isWorking: false)
        #expect(row.stateText == "Checking…")
        #expect(row.isOn == false)
        #expect(row.note == nil)
    }

    @Test func anEnabledAgentSaysWhatTheNextLoginDoes() {
        let row = content(.enabled, count: 2)
        #expect(row.isOn)
        #expect(
            row.note
                == "At your next login, stacklyd starts, then 2 instances set to start at login.")
        #expect(row.showsOpenLoginItems == false)
    }

    @Test func theNextLoginSentenceCountsOneInstanceInTheSingular() {
        #expect(
            StartupRowContent.nextLogin(atLoginCount: 1)
                == "At your next login, stacklyd starts, then 1 instance set to start at login.")
        #expect(
            StartupRowContent.nextLogin(atLoginCount: 0) == "At your next login, stacklyd starts.")
    }

    @Test func anAgentThatNeedsApprovalIsOnAndOffersLoginItems() {
        let row = content(.requiresApproval)
        #expect(row.isOn)
        #expect(row.showsOpenLoginItems)
        #expect(row.note == AutostartText.agentNeedsApproval)
    }

    @Test func anAgentThatIsNotRegisteredCanBeTurnedOn() {
        let row = content(.notRegistered)
        #expect(row.isOn == false)
        #expect(row.toggleDisabledReason == nil)
        #expect(row.note == AutostartText.agentOff)
    }

    @Test func anAgentTheUserTurnedOffInMacOSPointsToLoginItems() {
        let row = content(.disabled)
        #expect(row.isOn == false)
        #expect(row.showsOpenLoginItems)
        #expect(row.toggleDisabledReason == StartupCopy.turnOnInLoginItems)
        #expect(row.note == AutostartText.agentOff)
    }

    @Test func anAgentMacOSCannotFindGivesTheMoveOrReinstallReason() {
        let row = content(.notFound)
        #expect(row.toggleDisabledReason == StartupCopy.notFound)
    }

    @Test func anUnavailableAgentGivesItsStateAsTheReasonAndNoNote() {
        let row = content(.unavailable, reason: .rootOverride)
        #expect(row.stateText == "Unavailable · STACKLY_ROOT is set")
        #expect(row.toggleDisabledReason == row.stateText)
        #expect(row.note == nil)
    }

    @Test func turningOffWaitsForTheTurnOffAction() {
        #expect(content(.enabled).toggleDisabledReason == StartupCopy.turnOffPending)
    }

    @Test func aCallInFlightDisablesTheToggleWithAReason() {
        #expect(content(.enabled, isWorking: true).toggleDisabledReason == "Working on it…")
        #expect(content(.notRegistered, isWorking: true).toggleDisabledReason == "Working on it…")
    }
}
