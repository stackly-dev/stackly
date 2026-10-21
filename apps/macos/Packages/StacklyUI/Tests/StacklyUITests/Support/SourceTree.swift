import Foundation
import Testing

/// The Swift sources of the macOS app, read by the source rule tests.
enum SourceTree {
    /// The repository root, found from this file's path. Builds must never remap source
    /// paths (`-file-prefix-map`), or this breaks.
    static let repositoryRoot = URL(filePath: #filePath)
        .deletingLastPathComponent()  // SourceTree.swift
        .deletingLastPathComponent()  // Support
        .deletingLastPathComponent()  // StacklyUITests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // StacklyUI
        .deletingLastPathComponent()  // Packages
        .deletingLastPathComponent()  // macos
        .deletingLastPathComponent()  // apps

    /// The folders the source rules cover, relative to the repository root.
    static let scannedFolders = [
        "apps/macos/Packages/StacklyUI/Sources",
        "apps/macos/Stackly",
    ]

    /// Every `.swift` file under ``scannedFolders``, sorted by path.
    static func swiftFiles() throws -> [SourceFile] {
        try scannedFolders.flatMap { folder in
            try files(under: folder).filter { $0.hasSuffix(".swift") }
        }
        .sorted()
        .map { path in
            let text = try String(contentsOf: url(of: path), encoding: .utf8)
            return SourceFile(path: path, text: text)
        }
    }

    /// The paths, relative to the repository root, of every file under `folder`.
    static func files(under folder: String) throws -> [String] {
        let folderURL = url(of: folder)
        let enumerator = try #require(
            FileManager.default.enumerator(at: folderURL, includingPropertiesForKeys: nil)
        )
        let rootPath = repositoryRoot.path(percentEncoded: false)
        return enumerator.compactMap { item in
            guard let fileURL = item as? URL, fileURL.hasDirectoryPath == false else {
                return nil
            }
            return String(fileURL.path(percentEncoded: false).dropFirst(rootPath.count))
        }
    }

    /// The URL of a path relative to the repository root.
    static func url(of path: String) -> URL {
        repositoryRoot.appending(path: path)
    }
}

/// One Swift source file, with its line comments removed so prose that names a
/// forbidden API does not count.
struct SourceFile {
    /// The path relative to the repository root.
    let path: String
    /// The file's lines, each with any `//` comment cut off. Line numbers start at 1.
    let codeLines: [String]

    init(path: String, text: String) {
        self.path = path
        codeLines = text.split(separator: "\n", omittingEmptySubsequences: false)
            .map { Self.removingLineComment(from: String($0)) }
    }

    /// Whether the first line of code (comments and blank lines skipped) is `#if DEBUG`.
    var startsWithIfDebug: Bool {
        guard let first = codeLines.first(where: { $0.trimmingCharacters(in: .whitespaces) != "" })
        else { return false }
        return first.trimmingCharacters(in: .whitespaces) == "#if DEBUG"
    }

    /// The numbers of the lines that contain `text`; with `outsideIfDebug` only those
    /// outside every `#if DEBUG` branch. The `#else` branch of `#if DEBUG` is outside; any
    /// other `#if` keeps the state of the code around it.
    func lineNumbers(matching text: String, outsideIfDebug: Bool) -> [Int] {
        // One entry per open `#if`: whether its current branch is the `#if DEBUG` branch,
        // and whether the lines inside it are debug only.
        var stack: [(isDebugConditional: Bool, isDebugOnly: Bool)] = []
        var numbers: [Int] = []
        for (index, line) in codeLines.enumerated() {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed == "#if DEBUG" {
                stack.append((true, true))
            } else if trimmed.hasPrefix("#if ") {
                stack.append((false, stack.last?.isDebugOnly ?? false))
            } else if trimmed == "#else" || trimmed.hasPrefix("#elseif") {
                if let top = stack.popLast() {
                    stack.append(
                        (top.isDebugConditional, top.isDebugConditional ? false : top.isDebugOnly))
                }
            } else if trimmed == "#endif" {
                _ = stack.popLast()
            } else if line.contains(text) {
                if outsideIfDebug == false || (stack.last?.isDebugOnly ?? false) == false {
                    numbers.append(index + 1)
                }
            }
        }
        return numbers
    }

    /// Cuts `line` at the first `//` that is outside a string literal.
    static func removingLineComment(from line: String) -> String {
        var isInString = false
        var isEscaped = false
        var previous: Character?
        for index in line.indices {
            let character = line[index]
            if isInString {
                if isEscaped {
                    isEscaped = false
                } else if character == "\\" {
                    isEscaped = true
                } else if character == "\"" {
                    isInString = false
                }
            } else if character == "\"" {
                isInString = true
            } else if character == "/", previous == "/" {
                return String(line[..<line.index(before: index)])
            }
            previous = character
        }
        return line
    }
}
