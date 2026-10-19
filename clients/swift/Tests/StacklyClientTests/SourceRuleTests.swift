import Foundation
import Testing

/// The GUI never starts a process and the client never opens a network
/// socket (spec 0013 AC-23): forbidden patterns in non test code, matched with
/// simple word boundaries. Line comments are skipped.
struct SourceRuleTests {
    nonisolated struct Rule: CustomTestStringConvertible, Sendable {
        let name: String
        let folders: [String]
        let patterns: [String]
        let forbiddenExamples: [String]
        let allowedExamples: [String]

        var testDescription: String { name }

        func regexes() throws -> [Regex<AnyRegexOutput>] {
            try patterns.map { try Regex($0).wordBoundaryKind(.simple) }
        }
    }

    nonisolated static let guiSources = [
        "clients/swift/Sources", "apps/macos/Packages/StacklyUI/Sources", "apps/macos/Stackly",
    ]

    nonisolated static let rules = [
        Rule(
            name: "no process spawn in the GUI",
            folders: guiSources,
            patterns: [
                #"\bProcess\("#, #"\bProcess\.run\b"#, #"\bNSTask\b"#, #"\bposix_spawn"#,
                #"\bfork\("#, #"\bexecv"#, #"(?:^|[^.\w])system\("#,
            ],
            forbiddenExamples: [
                "let task = Process()", "try Process.run(url, arguments: [])", "NSTask()",
                "posix_spawn(&pid, path, nil, nil, argv, envp)", "let pid = fork()",
                "execv(path, argv)", "system(\"ls\")",
            ],
            allowedExamples: [
                "ProcessInfo.processInfo.processIdentifier", ".font(.system(.body))",
                "Font.system(size: 12)",
            ]
        ),
        Rule(
            name: "no network sockets in the client",
            folders: ["clients/swift/Sources"],
            patterns: [
                #"\bNWConnection\b"#, #"\bNWListener\b"#, #"\bURLSession\b"#, #"\bCFSocket"#,
                #"\bAF_INET"#,
            ],
            forbiddenExamples: [
                "NWConnection(host: h, port: p, using: .tcp)", "NWListener(using: .tcp)",
                "URLSession.shared", "CFSocketCreate(nil, 0, 0, 0, 0, nil, nil)",
                "socket(AF_INET, SOCK_STREAM, 0)", "socket(AF_INET6, SOCK_STREAM, 0)",
            ],
            allowedExamples: ["socket(AF_UNIX, SOCK_STREAM, 0)"]
        ),
    ]

    @Test(arguments: rules)
    func sourcesFollowTheRule(rule: Rule) throws {
        let regexes = try rule.regexes()
        var scanned = 0
        for folder in rule.folders {
            let base = Fixture.repositoryRoot.appending(path: folder)
            let enumerator = FileManager.default.enumerator(
                at: base, includingPropertiesForKeys: nil)
            while let url = enumerator?.nextObject() as? URL {
                guard url.pathExtension == "swift" else { continue }
                scanned += 1
                let text = try String(contentsOf: url, encoding: .utf8)
                for (index, line) in text.split(separator: "\n", omittingEmptySubsequences: false)
                    .enumerated()
                {
                    let code = Self.removingLineComment(String(line))
                    for regex in regexes where code.contains(regex) {
                        Issue.record(
                            "\(folder)/\(url.lastPathComponent):\(index + 1) breaks “\(rule.name)”")
                    }
                }
            }
        }
        #expect(scanned > 0, "no Swift files under \(rule.folders)")
    }

    @Test(arguments: rules)
    func ruleCatchesItsForbiddenExamples(rule: Rule) throws {
        let regexes = try rule.regexes()
        for example in rule.forbiddenExamples {
            #expect(regexes.contains { example.contains($0) }, "missed: \(example)")
        }
        for example in rule.allowedExamples {
            #expect(regexes.contains { example.contains($0) } == false, "caught: \(example)")
        }
    }

    /// The line without a `//` comment that is not inside a string literal.
    static func removingLineComment(_ line: String) -> String {
        var inString = false
        var escaped = false
        var previous: Character?
        var result = ""
        for character in line {
            if inString {
                if escaped {
                    escaped = false
                } else if character == "\\" {
                    escaped = true
                } else if character == "\"" {
                    inString = false
                }
            } else if character == "\"" {
                inString = true
            } else if character == "/", previous == "/" {
                result.removeLast()
                return result
            }
            result.append(character)
            previous = character
        }
        return result
    }
}
