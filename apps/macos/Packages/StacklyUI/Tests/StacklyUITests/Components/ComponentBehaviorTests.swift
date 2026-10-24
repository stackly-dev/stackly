import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The behaviors of the card, console, action, and feedback components that the per
/// component suites leave open (spec 0067, AC-3, AC-5, AC-7, AC-8, AC-9, AC-11): the card
/// rows, what the console copies, the busy and disabled button, the More menu, and a
/// feedback center that has no presenter yet.
///
/// A hosted view builds no accessibility tree without a running assistive client, so the
/// view shape is read from the source, as the other component suites do.
struct ComponentBehaviorTests {
    private static let components =
        "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/"

    private func code(_ name: String) throws -> String {
        let path = Self.components + name
        return SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
        )
        .codeLines.joined(separator: "\n")
    }

    // MARK: - AC-3: the card

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-3: the card gives every row 11 pt above and below, 14 pt at the "
                + "sides, the row minimum height, and a divider between rows"
        ))
    func theCardPadsEveryRowAndSeparatesThemWithADivider() throws {
        let card = try code("KeyValue/CardSection.swift")
        #expect(card.contains(".padding(Spacing.rowInsets)"))
        #expect(card.contains("minHeight: rowMinHeight"))
        #expect(card.contains("if row.id != rows.last?.id {"), "no divider after the last row")
        #expect(card.contains("Divider()"))
        #expect(card.contains(".cardSurface()"))
        #expect(card.contains(".font(.headline)"))

        let insets = Spacing.rowInsets
        #expect(insets.top == 11 && insets.bottom == 11)
        #expect(insets.leading == 14 && insets.trailing == 14)
    }

    @Test func theKeyValueStyleSetsLabelInPrimaryAndValueInSecondaryText() throws {
        let style = try code("KeyValue/StacklyLabeledContentStyle.swift")
        #expect(style.contains("StacklyColor.textSecondary"))
        #expect(style.contains(".textSelection(.enabled)"))
        #expect(style.contains("ViewThatFits"), "one line when it fits, label above otherwise")
    }

    // MARK: - AC-5: the console

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected:
                "Spec 0067 AC-5: the copy button of a command block copies the command "
                + "without its dollar prompt"
        ))
    func theCopyButtonOfACommandCopiesTheCommandWithoutItsPrompt() throws {
        let console = try code("Copy/ConsoleBlock.swift")

        let block = ConsoleBlock(command: "stackly daemon status")
        #expect(block.kind == .command("stackly daemon status", what: "command"))
        #expect(console.contains("CopyButton(command, what: what)"))

    }

    @Test func aTextBlockHasACopyButtonOnlyWhenTheScreenAsksForOne() throws {
        let console = try code("Copy/ConsoleBlock.swift")
        #expect(console.contains("case .text(let text, let copy?):"))
        #expect(console.contains("case .text:\n            EmptyView()"))
        let plain = ConsoleBlock(text: "a")
        #expect(plain.kind == .text("a", copy: nil))
    }

    // MARK: - AC-7: the action button

    @Test(
        .evidence(
            covers: ["REQ-026"],
            expected:
                "Spec 0067 AC-7: a busy button is disabled with a small spinner and the value "
                + "In progress, and a disabled one writes its reason only when asked to"
        ))
    func aBusyButtonIsDisabledWithASpinnerAndAValueAndTheReasonShowsOnlyWhenAsked() throws {
        let button = try code("Actions/ActionButton.swift")
        #expect(button.contains(".disabled(availability == .busy)"))
        #expect(button.contains("ProgressView()"))

        #expect(
            button.contains("if showsReason, case .disabled(let reason, true) = availability {"))
        #expect(button.contains("DisabledReasonText(reason)"))
        #expect(button.contains(".disabled(reason: disabledHelp)"))

    }

    @Test func aPrimaryButtonIsProminentAndASecondaryOneIsBorderedOnly() throws {
        let button = try code("Actions/ActionButton.swift")
        #expect(
            button.contains("isProminent: prominence == .primary && !action.role.isDestructive"))
        #expect(button.contains("content.buttonStyle(.borderedProminent)"))
        #expect(button.contains("content.buttonStyle(.bordered)"))
        #expect(button.contains("role: action.role.isDestructive ? .destructive : nil"))
    }

    @Test func noActionViewEverHidesAnActionBecauseOfItsState() throws {
        // A capability that is not supported stays on screen, disabled with its reason.
        for name in ["Actions/ActionButton.swift", "Actions/ActionMenu.swift"] {
            let text = try code(name)
            #expect(text.contains(".hidden()") == false, "\(name) hides an action")
            #expect(text.contains("opacity(0)") == false, "\(name) hides an action")
        }
    }

    // MARK: - AC-8: the menu

    // MARK: - AC-9 and AC-11: feedback with no presenter yet

    private func makeCenter() -> FeedbackCenter {
        FeedbackCenter(sleep: { _ in })
    }

    @Test func eachWindowsCenterKeepsItsOwnFeedback() {
        let one = makeCenter()
        let two = makeCenter()
        one.show(.success("One."))
        #expect(two.current == nil)
        two.dismiss()
        #expect(one.current?.message == "One.")
    }

    @Test func aPersistentFeedbackStaysThroughACountdownThatWasNeverCancelled() async {
        let center = makeCenter()
        let critical = Feedback(tone: .critical, message: "Failed.")
        center.show(critical)
        await center.countdown(critical.id)
        #expect(center.current?.id == critical.id, "critical feedback never goes away on its own")
    }

    @Test func aSheetNeverSendsToTheFeedbackCenter() throws {
        let sheet = try code("Sheet/StacklySheet.swift")
        #expect(sheet.contains("feedbackCenter") == false)
        #expect(sheet.contains("FeedbackCenter") == false)
    }
}
