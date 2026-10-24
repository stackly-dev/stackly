import AppKit
import Testing

@testable import StacklyUI

/// The confirmation pattern of spec 0012 (AC-15): one native alert per request, Cancel
/// as the default button of a destructive request, and the copy rules.
///
/// What Return and Escape do in the running app is checked in the Debug Design Gallery
/// (see `apps/macos/design.md`, Confirmation); these tests pin the alert those keys act on.
struct ConfirmationTests {
    /// The remove instance confirmation of the mockup.
    static func removeRequest(perform: @escaping @MainActor () -> Void = {})
        -> ConfirmationRequest
    {
        ConfirmationRequest(
            title: "Remove “pg17-dev”?",
            message:
                "The instance is unregistered and its process stopped. The data directory (1.24 GB) stays on disk.",
            actionTitle: "Remove",
            role: .destructive,
            perform: perform
        )
    }

    /// The bring down environment confirmation of the mockup.
    static func bringDownRequest() -> ConfirmationRequest {
        ConfirmationRequest(
            title: "Bring down “erp-development”?",
            message:
                "Releases the lease and stops only instances nothing else needs. Shared and manually started instances keep running. Data is kept.",
            actionTitle: "Bring Down",
            role: .standard,
            perform: {}
        )
    }

    static func request(_ role: ConfirmationRequest.Role) -> ConfirmationRequest {
        switch role {
        case .destructive: removeRequest()
        case .standard: bringDownRequest()
        }
    }

    @Test func destructiveAlertListsCancelFirstWithoutAKeyEquivalent() {
        let confirmation = ConfirmationAlert(request: Self.removeRequest())
        let alert = confirmation.alert
        #expect(alert.messageText == "Remove “pg17-dev”?")
        #expect(alert.informativeText.hasPrefix("The instance is unregistered"))
        #expect(alert.alertStyle == .critical)
        #expect(alert.buttons.map(\.title) == ["Cancel", "Remove"])
        #expect(confirmation.cancelButton.keyEquivalent.isEmpty)
        #expect(confirmation.actionButton.keyEquivalent == "")
        #expect(confirmation.actionButton.hasDestructiveAction)
        #expect(confirmation.cancelButton.hasDestructiveAction == false)
    }

    @Test func standardAlertListsTheActionFirstWithoutAKeyEquivalent() {
        let confirmation = ConfirmationAlert(request: Self.bringDownRequest())
        let alert = confirmation.alert
        #expect(alert.messageText == "Bring down “erp-development”?")
        #expect(alert.alertStyle == .informational)
        #expect(alert.buttons.map(\.title) == ["Bring Down", "Cancel"])
        #expect(confirmation.actionButton.keyEquivalent.isEmpty)
        #expect(confirmation.cancelButton.keyEquivalent.isEmpty)
        #expect(confirmation.actionButton.hasDestructiveAction == false)
    }

    @Test(arguments: ConfirmationRequest.Role.allCases)
    func onlyTheActionButtonConfirms(role: ConfirmationRequest.Role) throws {
        let confirmation = ConfirmationAlert(request: Self.request(role))
        let buttons = confirmation.alert.buttons
        let actionIndex = try #require(buttons.firstIndex { $0 === confirmation.actionButton })
        for index in buttons.indices {
            let response = NSApplication.ModalResponse(
                NSApplication.ModalResponse.alertFirstButtonReturn.rawValue + index
            )
            #expect(confirmation.isConfirmed(response) == (index == actionIndex))
        }
        for response: NSApplication.ModalResponse in [.cancel, .abort, .stop, .continue] {
            #expect(confirmation.isConfirmed(response) == false)
        }
    }

    @Test func everyRequestHasItsOwnIdentity() {
        let first = Self.removeRequest()
        let second = Self.removeRequest()
        let copy = first
        #expect(first.id != second.id)
        #expect(copy.id == first.id)
    }

    // MARK: Copy rules

    /// One request copy and the copy problems it must report.
    nonisolated struct CopyCase: CustomTestStringConvertible, Sendable {
        let title: String
        let message: String
        let actionTitle: String
        let problems: [ConfirmationCopyProblem]

        var testDescription: String { "“\(title)” / “\(actionTitle)”" }
    }

    nonisolated static let copyCases = [
        CopyCase(
            title: "Remove “pg17-dev”?",
            message: "Data is kept.",
            actionTitle: "Remove",
            problems: []
        ),
        CopyCase(
            title: "Bring down “erp-development”?",
            message: "Data is kept.",
            actionTitle: "Bring Down",
            problems: []
        ),
        CopyCase(
            title: "Remove PostgreSQL 16.10?",
            message: "Instance data is never touched.",
            actionTitle: "Remove",
            problems: []
        ),
        CopyCase(
            title: "",
            message: "Data is kept.",
            actionTitle: "Remove",
            problems: [.emptyTitle, .actionTitleNotInTitle]
        ),
        CopyCase(
            title: "Remove “pg17-dev”",
            message: "Data is kept.",
            actionTitle: "Remove",
            problems: [.titleIsNotAQuestion]
        ),
        CopyCase(
            title: "Remove “pg17-dev”?",
            message: "  ",
            actionTitle: "Remove",
            problems: [.emptyMessage]
        ),
        CopyCase(
            title: "Remove “pg17-dev”?",
            message: "Data is kept.",
            actionTitle: "",
            problems: [.emptyActionTitle]
        ),
        CopyCase(
            title: "Remove “pg17-dev”?",
            message: "Data is kept.",
            actionTitle: "OK",
            problems: [.genericActionTitle]
        ),
        CopyCase(
            title: "Remove “pg17-dev”?",
            message: "Data is kept.",
            actionTitle: "Yes",
            problems: [.genericActionTitle]
        ),
        CopyCase(
            title: "Remove “pg17-dev”?",
            message: "Data is kept.",
            actionTitle: "Delete",
            problems: [.actionTitleNotInTitle]
        ),
    ]

    @Test(arguments: copyCases)
    func copyRulesReportEveryProblem(copy: CopyCase) {
        let request = ConfirmationRequest(
            title: copy.title,
            message: copy.message,
            actionTitle: copy.actionTitle,
            role: .destructive,
            perform: {}
        )
        #expect(request.copyProblems == copy.problems)
    }

    #if DEBUG
        @Test func gallerySamplesFollowTheCopyRulesForBothRoles() {
            let samples = ConfirmationSample.all
            #expect(Set(samples.map(\.role)) == Set(ConfirmationRequest.Role.allCases))
            for sample in samples {
                #expect(sample.request {}.copyProblems.isEmpty, "\(sample.title)")
            }
        }
    #endif
}
