import Foundation
import Testing

@testable import StacklyUI

/// VoiceOver labels, keyboard reach, and where a typed value may live, checked on the
/// source of the secret views (spec 0011 AC-19, *GUI*, Aksesibilitas). A hosted view builds
/// no accessibility tree without a running assistive client, so the labels are read from the
/// code, as the status views do; the logic they depend on has its own model tests.
struct SecretViewsAccessibilityTests {
    private static let folder = "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Secrets"

    private func source(_ name: String) throws -> SourceFile {
        let path = Self.folder + "/" + name
        return SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8))
    }

    private func sources() throws -> [SourceFile] {
        try SourceTree.files(under: Self.folder).filter { $0.hasSuffix(".swift") }.map {
            SourceFile(
                path: $0, text: try String(contentsOf: SourceTree.url(of: $0), encoding: .utf8))
        }
    }

    @Test func aHiddenValueIsLabelledPasswordHiddenWithItsReferences() throws {
        let file = try source("RevealableSecretText.swift")
        #expect(
            file.codeLines.contains { $0.contains(#".accessibilityLabel("Password, hidden")"#) })
        #expect(
            file.codeLines.contains {
                $0.contains(".accessibilityValue(model.references.joined(separator:")
            })
    }

    @Test func showHideAndCopyAreNativeButtonsWithTheirOwnLabels() throws {
        let file = try source("RevealableSecretText.swift")
        let code = file.codeLines.joined(separator: "\n")
        #expect(code.contains(#"Button(model.isShown ? "Hide" : "Show")"#))
        #expect(code.contains(#"Button("Copy")"#))
        for label in ["Hide Password", "Show Password", "Copy Password"] {
            #expect(code.contains("\"\(label)\""), "missing the label \(label)")
        }
    }

    @Test func theFieldUsesNativeSecureInputAndNamesEachControl() throws {
        let code = try source("SecretField.swift").codeLines.joined(separator: "\n")
        #expect(code.contains("SecureField(title, text: $draft.text)"))
        #expect(code.contains(#".accessibilityLabel("\(title), shown")"#))
        #expect(
            code.contains(
                #".accessibilityLabel(showsTypedText ? "Hide \(title)" : "Show \(title)")"#))
        #expect(code.contains(#".accessibilityLabel("\(title), source")"#))
    }

    @Test func theSheetHasKeyboardDefaultAndCancelAndAHeader() throws {
        let code = try source("SecretEntrySheet.swift").codeLines.joined(separator: "\n")
        #expect(code.contains(".keyboardShortcut(.defaultAction)"))
        #expect(code.contains(".keyboardShortcut(.cancelAction)"))
        #expect(code.contains(".accessibilityAddTraits(.isHeader)"))
        #expect(code.contains(".stacklyConfirmation(item: $confirmation)"))
    }

    @Test func noSecretViewActsOnATapGestureWithoutAButton() throws {
        for file in try sources() {
            #expect(
                !file.codeLines.contains { $0.contains("onTapGesture") },
                "\(file.path) uses a tap gesture, which the keyboard cannot reach")
        }
    }

    @Test func aTypedValueNeverReachesPersistedStateOrALog() throws {
        let forbidden = [
            "UserDefaults", "@AppStorage", "@SceneStorage", "NSUbiquitousKeyValueStore", "Logger(",
            "os_log", "print(", "NSLog", "@Query",
        ]
        for file in try sources() {
            for word in forbidden {
                #expect(
                    !file.codeLines.contains { $0.contains(word) },
                    "\(file.path) uses \(word)")
            }
        }
    }

    @Test func theModelsHoldNoTypedValue() throws {
        // The entry model and the reveal model are the only `@Observable` types here. The
        // entry model never stores a draft; the reveal model stores the shown text only.
        let entry = try source("SecretEntryModel.swift")
        #expect(
            !entry.codeLines.contains {
                $0.contains("SecretDraft") && ($0.contains(" var ") || $0.contains(" let "))
            })
        let reveal = try source("RevealModel.swift").codeLines.joined(separator: "\n")
        #expect(reveal.contains("public private(set) var shownText: String?"))
    }
}
