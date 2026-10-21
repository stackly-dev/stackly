import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The accessibility identifiers of the shared components (spec 0067, AC-20, *Accessibility
/// identifier*): the table values, the `<scope>.<part>[.<key>]` form feature 68 agreed on,
/// and that no two parts share a value.
struct AccessibilityIdentifierTests {
    /// The constants of the spec table and the value each one must have.
    private static let fixed: [(constant: String, value: String)] = [
        (AccessibilityID.Feedback.card, "feedback.card"),
        (AccessibilityID.Feedback.action, "feedback.action"),
        (AccessibilityID.Feedback.dismiss, "feedback.dismiss"),
        (AccessibilityID.Sheet.cancel, "sheet.cancel"),
        (AccessibilityID.Sheet.primary, "sheet.primary"),
        (AccessibilityID.Inspector.toggle, "inspector.toggle"),
        (AccessibilityID.Inspector.placeholder, "inspector.placeholder"),
        (AccessibilityID.Action.more, "action.more"),
        (AccessibilityID.Progress.view, "progress.view"),
        (AccessibilityID.Log.list, "log.list"),
        (AccessibilityID.Diff.list, "diff.list"),
    ]

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "Spec 0067 AC-20: every fixed identifier has the value of the spec table"
        ))
    func everyFixedIdentifierHasTheValueOfTheSpecTable() {
        for (constant, value) in Self.fixed {
            #expect(constant == value)
        }
    }

    @Test func everyFixedIdentifierHasTheScopePartForm() {
        for (constant, _) in Self.fixed {
            let parts = constant.split(separator: ".", omittingEmptySubsequences: false)
            #expect(parts.count == 2, "\(constant) is <scope>.<part>")
            #expect(parts.allSatisfy { !$0.isEmpty }, "\(constant) has an empty part")
            #expect(parts[0].allSatisfy { $0.isLowercase }, "\(constant) scope is lower case")
        }
    }

    @Test func noTwoFixedIdentifiersShareAValue() {
        let values = Self.fixed.map(\.constant)
        #expect(Set(values).count == values.count)
    }

    @Test func aKeyedIdentifierPutsTheKeyAsTheLastPart() {
        #expect(AccessibilityID.Action.button("stop") == "action.button.stop")
        #expect(AccessibilityID.Checklist.row("port") == "checklist.row.port")
        #expect(AccessibilityID.Copy.button("endpoint") == "copy.button.endpoint")
    }

    @Test func aKeyIsPassedThroughUnchangedEvenWhenItHoldsADot() {
        // The key is only a stable data key; the helper does not escape or split it.
        #expect(AccessibilityID.Action.button("a.b") == "action.button.a.b")
        #expect(AccessibilityID.Checklist.row("") == "checklist.row.")
    }

    @Test func keyedIdentifiersOfDifferentKeysDiffer() {
        #expect(AccessibilityID.Action.button("start") != AccessibilityID.Action.button("stop"))
        #expect(AccessibilityID.Checklist.row("a") != AccessibilityID.Checklist.row("b"))
        #expect(AccessibilityID.Copy.button("a") != AccessibilityID.Copy.button("b"))
    }
}
