import Foundation
import ServiceManagement
import Testing

@testable import StacklyUI

/// The small helpers of the agent folder: the home folder, `~` paths, canonical paths,
/// the launch record, and the bundle check of the live adapter. None of them calls
/// `SMAppService` (spec 0015, AC-4, AC-11).
struct AgentSupportTests {
    // MARK: - UserHome

    @Test func theHomeIsWrittenAsATildeOnlyOnAComponentBoundary() {
        #expect(UserHome.tildePath("/Users/a", home: "/Users/a") == "~")
        #expect(
            UserHome.tildePath("/Users/a/.local/bin/stackly", home: "/Users/a")
                == "~/.local/bin/stackly")
        #expect(UserHome.tildePath("/Users/ab/x", home: "/Users/a") == "/Users/ab/x")
        #expect(UserHome.tildePath("/Users/a", home: "/Users/a/") == "~")
        #expect(UserHome.tildePath("/elsewhere", home: "/Users/a") == "/elsewhere")
    }

    @Test func theHomeComesFromThePasswordDatabase() {
        let home = UserHome.current()
        #expect(home.path.hasPrefix("/"))
        #expect(home.path.isEmpty == false)
    }

    // MARK: - CanonicalPath

    @Test func aSymlinkResolvesToItsTargetAndKeepsPrivate() throws {
        let root = try TemporaryFolder()
        let target = root.url.appending(path: "target")
        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
        let link = root.url.appending(path: "link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: target)
        let resolved = CanonicalPath.resolve(link)
        #expect(resolved.lastPathComponent == "target")
        // `/var` is a link to `/private/var`; realpath keeps the long form.
        #expect(resolved.path.hasPrefix("/private/") || resolved.path.hasPrefix("/var") == false)
    }

    @Test func aMissingPathResolvesToItself() {
        let url = URL(filePath: "/no/such/folder")
        #expect(CanonicalPath.resolve(url) == url)
    }

    // MARK: - Launch record

    @Test func aMissingRecordDoesNotExist() throws {
        let root = try TemporaryFolder()
        #expect(LaunchRecordReader.read(at: root.url.appending(path: "none.json")) == .missing)
    }

    @Test func aRecordGivesItsLastLaunchTime() throws {
        let root = try TemporaryFolder()
        let file = root.url.appending(path: "background-agent.json")
        try Data(#"{"schema":1,"lastLaunchedAt":"2026-10-22T10:30:15.123456789Z"}"#.utf8).write(
            to: file)
        let status = LaunchRecordReader.read(at: file)
        #expect(status.exists)
        let expected = ISO8601DateFormatter.fractional.date(from: "2026-10-22T10:30:15.123Z")
        #expect(status.lastLaunchedAt == expected)
    }

    @Test func aRecordWithoutAValidTimeStillExists() throws {
        let root = try TemporaryFolder()
        let file = root.url.appending(path: "background-agent.json")
        for text in [#"{"lastLaunchedAt":"yesterday"}"#, "not json", #"{"schema":1}"#] {
            try Data(text.utf8).write(to: file)
            #expect(
                LaunchRecordReader.read(at: file)
                    == LaunchRecordStatus(exists: true, lastLaunchedAt: nil))
        }
    }

    @Test func aRecordOverFourKibHasNoTime() throws {
        let root = try TemporaryFolder()
        let file = root.url.appending(path: "background-agent.json")
        let padding = String(repeating: "x", count: 5000)
        try Data(
            #"{"lastLaunchedAt":"2026-10-22T10:30:15Z","pad":"\#(padding)"}"#.utf8
        ).write(to: file)
        #expect(
            LaunchRecordReader.read(at: file)
                == LaunchRecordStatus(exists: true, lastLaunchedAt: nil))
    }

    @Test func aSymlinkOrFolderIsNotARecord() throws {
        let root = try TemporaryFolder()
        let real = root.url.appending(path: "real.json")
        try Data(#"{"lastLaunchedAt":"2026-10-22T10:30:15Z"}"#.utf8).write(to: real)
        let link = root.url.appending(path: "link.json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: real)
        #expect(LaunchRecordReader.read(at: link) == .missing)
        #expect(LaunchRecordReader.read(at: root.url) == .missing)
    }

    // MARK: - The bundle check

    private func fakeBundle(
        in root: TemporaryFolder, name: String, plist: Bool = true, daemon: Bool = true
    ) throws -> URL {
        let bundle = root.url.appending(path: name)
        let manager = FileManager.default
        try manager.createDirectory(
            at: bundle.appending(path: AgentIdentifiers.launchAgentsDir),
            withIntermediateDirectories: true)
        try manager.createDirectory(
            at: bundle.appending(path: AgentIdentifiers.macosDir), withIntermediateDirectories: true
        )
        if plist {
            try Data().write(
                to: bundle.appending(path: AgentIdentifiers.launchAgentsDir)
                    .appending(path: AgentIdentifiers.daemonPlistFile))
        }
        if daemon {
            let path = bundle.appending(path: AgentIdentifiers.daemonBundleProgram)
            try Data("#!/bin/sh\n".utf8).write(to: path)
            try manager.setAttributes([.posixPermissions: 0o755], ofItemAtPath: path.path)
        }
        return bundle
    }

    @Test func aBundleWithTheRightIdentifierPlistAndDaemonIsLocated() throws {
        let root = try TemporaryFolder()
        let bundle = try fakeBundle(in: root, name: "Good.app")
        #expect(
            SMAppServiceAgent.locate(
                bundleURL: bundle, bundleIdentifier: AgentIdentifiers.appBundleID)
                == nil)
    }

    @Test func anotherIdentifierOrNoneIsNotBundled() throws {
        let root = try TemporaryFolder()
        let bundle = try fakeBundle(in: root, name: "Other.app")
        for identifier in ["com.example.other", nil] {
            #expect(
                SMAppServiceAgent.locate(bundleURL: bundle, bundleIdentifier: identifier)
                    == .notBundled(executable: bundle.path))
        }
    }

    @Test func aBundleWithoutThePlistOrTheDaemonHasNoHelper() throws {
        let root = try TemporaryFolder()
        let noPlist = try fakeBundle(in: root, name: "NoPlist.app", plist: false)
        let noDaemon = try fakeBundle(in: root, name: "NoDaemon.app", daemon: false)
        for bundle in [noPlist, noDaemon] {
            #expect(
                SMAppServiceAgent.locate(
                    bundleURL: bundle, bundleIdentifier: AgentIdentifiers.appBundleID)
                    == .helperMissing(bundle: bundle.path))
        }
    }

    @Test func theAdapterAnswersWithoutTheOSForAnUnbundledApp() async throws {
        let root = try TemporaryFolder()
        let bundle = try fakeBundle(in: root, name: "Other.app")
        let agent = SMAppServiceAgent(bundleURL: bundle, bundleIdentifier: nil)
        let canonical = CanonicalPath.resolve(bundle).path
        let probe = try await agent.status()
        #expect(probe == .notBundled(executable: canonical))
        #expect(try await agent.register() == .notBundled(executable: canonical))
        #expect(try await agent.relaunch() == .notBundled(executable: canonical))
    }

    @Test func theOSErrorKindsFollowTheHeaderConstants() {
        func kind(_ code: Int, domain: String = "SMAppServiceErrorDomain") -> AgentOsErrorKind {
            SMAppServiceAgent.kind(of: NSError(domain: domain, code: code))
        }
        #expect(kind(1) == .other)
        #expect(kind(kSMErrorAlreadyRegistered) == .alreadyRegistered)
        #expect(kind(kSMErrorLaunchDeniedByUser) == .launchDenied)
        #expect(kind(kSMErrorJobMustBeEnabled) == .mustBeEnabled)
        #expect(kind(kSMErrorInvalidSignature) == .invalidSignature)
        #expect(kind(kSMErrorJobNotFound) == .notFound)
        #expect(kind(kSMErrorJobPlistNotFound) == .notFound)
        #expect(kind(kSMErrorLaunchDeniedByUser, domain: "OtherDomain") == .other)
    }
}

/// A folder in the temporary directory that is removed when the value goes away.
final class TemporaryFolder {
    let url: URL

    init() throws {
        url = URL(filePath: NSTemporaryDirectory()).appending(path: "stackly-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    }

    deinit {
        try? FileManager.default.removeItem(at: url)
    }
}

extension ISO8601DateFormatter {
    fileprivate static let fractional: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()
}
