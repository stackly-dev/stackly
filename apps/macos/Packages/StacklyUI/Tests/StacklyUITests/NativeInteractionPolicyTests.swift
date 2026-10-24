import Foundation
import Testing

/// Guards the interaction scope approved on 2026-10-24 across the app and shared views.
@Suite(.serialized) nonisolated struct NativeInteractionPolicyTests {
    @Test func appSourcesUseNativeControlsWithoutCustomAssistiveInteraction() throws {
        let forbidden = [
            #"\.accessibility[A-Z]\w*\s*\("#,
            #"\bAccessibilityNotification\b"#,
            #"\b(ConfirmationKey|KeyboardShortcut|KeyEquivalent)\b"#,
            #"NSEvent\.addLocalMonitorForEvents.*\.keyDown"#,
            #"@(AccessibilityFocusState|FocusState)\b"#,
            #"\.(keyboardShortcut|focused|searchFocused|onKeyPress|onExitCommand|onMoveCommand)\s*\("#,
            #"\\\._?accessibility[A-Z]\w*"#,
            #"\bColorSchemeContrast\b"#,
            #"@(ScaledMetric)\b|\bshowsStatusSymbols\b"#,
        ]
        let patterns = try forbidden.map { try Regex($0) }
        let files = try SourceTree.swiftFiles()
        #expect(files.isEmpty == false)
        for file in files {
            for (index, line) in file.codeLines.enumerated() {
                for pattern in patterns where line.contains(pattern) {
                    Issue.record("\(file.path):\(index + 1) uses a removed interaction API")
                }
            }
        }
    }
}
