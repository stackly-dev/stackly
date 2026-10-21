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
        #expect(card.contains(".accessibilityAddTraits(.isHeader)"))
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
        #expect(console.contains("CopyButton(command, what: what, announcement: announcement)"))
        let block = ConsoleBlock(command: "stackly daemon status")
        #expect(
            block.kind
                == .command(
                    "stackly daemon status", what: "command", announcement: "Command copied."))
        #expect(block.spokenText.hasPrefix("$") == false)
    }

    @Test func aTextBlockHasACopyButtonOnlyWhenTheScreenAsksForOne() throws {
        let console = try code("Copy/ConsoleBlock.swift")
        #expect(console.contains("case .text(let text, let copy?):"))
        #expect(console.contains("case .text:\n            EmptyView()"))
        let plain = ConsoleBlock(text: "a", accessibilityLabel: "Output")
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
        #expect(button.contains(".accessibilityHidden(true)"), "the spinner is not read twice")
        #expect(button.contains(#".accessibilityValue("In progress")"#))
        #expect(
            button.contains("if showsReason, case .disabled(let reason, true) = availability {"))
        #expect(button.contains("DisabledReasonText(reason)"))
        #expect(button.contains(".disabled(reason: disabledHelp)"))
        #expect(
            button.contains(".accessibilityIdentifier(AccessibilityID.Action.button(action.id))"))
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

    @Test(
        .evidence(
            covers: ["REQ-026"],
            expected:
                "Spec 0067 AC-8: the More menu is labelled More with the ellipsis symbol, "
                + "separates its sections with dividers, and sets no keyboard shortcut"
        ))
    func theMoreMenuIsLabelledMoreSeparatesSectionsAndSetsNoShortcut() throws {
        let menu = try code("Actions/ActionMenu.swift")
        #expect(menu.contains(#"title: String = "More""#))
        #expect(menu.contains(#"symbol: String = "ellipsis.circle""#))
        #expect(menu.contains("AccessibilityID.Action.more"))
        #expect(menu.contains("Divider()"))
        #expect(menu.contains("role: action.role.isDestructive ? .destructive : nil"))
        #expect(menu.contains("keyboardShortcut") == false, "shortcuts belong to spec 0069")
    }

    // MARK: - AC-9 and AC-11: feedback with no presenter yet

    private final class Announcements {
        var texts: [String] = []
    }

    private func makeCenter(_ announcements: Announcements) -> FeedbackCenter {
        FeedbackCenter(announce: { announcements.texts.append($0) }, sleep: { _ in })
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-11: feedback sent before a presenter is on screen stays in the "
                + "center and is announced once"
        ))
    func feedbackSentBeforeAnyPresenterStaysInTheCenterAndIsAnnouncedOnce() {
        let announcements = Announcements()
        let center = makeCenter(announcements)
        center.show(.success("Setup complete."))
        // No presenter has looked yet; reading it later still finds the card.
        #expect(center.current?.message == "Setup complete.")
        #expect(announcements.texts == ["Setup complete."])
        _ = center.current
        #expect(announcements.texts.count == 1, "looking at it again never announces again")
    }

    @Test func eachWindowsCenterKeepsItsOwnFeedback() {
        let first = Announcements()
        let second = Announcements()
        let one = makeCenter(first)
        let two = makeCenter(second)
        one.show(.success("One."))
        #expect(two.current == nil)
        #expect(second.texts.isEmpty)
        two.dismiss()
        #expect(one.current?.message == "One.")
    }

    @Test func aPersistentFeedbackStaysThroughACountdownThatWasNeverCancelled() async {
        let announcements = Announcements()
        let center = makeCenter(announcements)
        let critical = Feedback(tone: .critical, message: "Failed.")
        center.show(critical)
        await center.countdown(critical.id)
        #expect(center.current?.id == critical.id, "critical feedback never goes away on its own")
    }

    @Test func dismissingAnAlreadyEmptyCenterChangesNothingAndAnnouncesNothing() {
        let announcements = Announcements()
        let center = makeCenter(announcements)
        center.dismiss()
        #expect(center.current == nil)
        #expect(announcements.texts.isEmpty)
    }

    @Test func aSheetNeverSendsToTheFeedbackCenter() throws {
        let sheet = try code("Sheet/StacklySheet.swift")
        #expect(sheet.contains("feedbackCenter") == false)
        #expect(sheet.contains("FeedbackCenter") == false)
    }
}
