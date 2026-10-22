import Foundation
import Testing

@testable import StacklyUI

/// The live command line tool in a temporary home: install, repair, remove, and every state
/// (spec 0015, AC-14). The tool writes only under `~/.local`, never a shell file.
struct CommandLineToolTests {
    /// A temporary home and the bundles a test links to.
    final class Home {
        let folder: TemporaryFolder
        var home: URL { folder.url.appending(path: "home") }
        var commandLink: URL { home.appending(path: ".local/bin/stackly") }
        var manFolder: URL { home.appending(path: ".local/share/man/man1") }

        init() throws {
            folder = try TemporaryFolder()
            try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        }

        /// A bundle `<name>/Stackly.app` with the command and two manual pages; the command is
        /// left out when `withTool` is false.
        func bundle(_ name: String, withTool: Bool = true) throws -> URL {
            let bundle = folder.url.appending(path: name).appending(path: "Stackly.app")
            let manager = FileManager.default
            let macos = bundle.appending(path: "Contents/MacOS")
            let man = bundle.appending(path: "Contents/Resources/man/man1")
            try manager.createDirectory(at: macos, withIntermediateDirectories: true)
            try manager.createDirectory(at: man, withIntermediateDirectories: true)
            if withTool {
                let tool = macos.appending(path: "stackly-cli")
                try Data("#!/bin/sh\n".utf8).write(to: tool)
                try manager.setAttributes([.posixPermissions: 0o755], ofItemAtPath: tool.path)
            }
            for page in ["stackly.1", "stackly-daemon.1"] {
                try Data(".TH x 1\n".utf8).write(to: man.appending(path: page))
            }
            return CanonicalPath.resolve(bundle)
        }

        func tool(for bundle: URL) -> CommandLineTool {
            CommandLineTool(home: CanonicalPath.resolve(home), bundle: bundle)
        }

        /// Every path under the home, relative to it.
        func paths() -> Set<String> {
            let root = CanonicalPath.resolve(home).path
            let enumerator = FileManager.default.enumerator(atPath: root)
            return Set((enumerator?.allObjects as? [String]) ?? [])
        }
    }

    @Test func aFreshHomeIsNotInstalledAndTheBundleHasItsCommand() async throws {
        let rig = try Home()
        let bundle = try rig.bundle("a")
        let tool = rig.tool(for: bundle)
        #expect(tool.bundledTool == bundle.appending(path: "Contents/MacOS/stackly-cli"))
        let status = await tool.status()
        #expect(status.command == .notInstalled)
        #expect(status.manPages.map(\.name) == ["stackly-daemon.1", "stackly.1"])
        #expect(status.manPages.allSatisfy { $0.state == .notInstalled })
    }

    @Test func installLinksTheCommandAndEveryPageAndMakesOnlyTheFolders() async throws {
        let rig = try Home()
        let bundle = try rig.bundle("a")
        let tool = rig.tool(for: bundle)
        try await tool.install()
        let status = await tool.status()
        #expect(status.command == .installed)
        #expect(status.commandTarget == bundle.appending(path: "Contents/MacOS/stackly-cli").path)
        #expect(status.manPages.allSatisfy { $0.state == .installed })
        #expect(
            rig.paths() == [
                ".local", ".local/bin", ".local/bin/stackly", ".local/share", ".local/share/man",
                ".local/share/man/man1", ".local/share/man/man1/stackly-daemon.1",
                ".local/share/man/man1/stackly.1",
            ])
        let attributes = try FileManager.default.attributesOfItem(
            atPath: rig.commandLink.deletingLastPathComponent().path)
        #expect((attributes[.posixPermissions] as? Int) == 0o755)
    }

    @Test func installLeavesShellFilesAndEverythingElseAlone() async throws {
        let rig = try Home()
        let files = [".zshrc", ".zprofile", ".bash_profile", ".config/fish/config.fish"]
        for file in files {
            let url = rig.home.appending(path: file)
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data("# mine\n".utf8).write(to: url)
        }
        let before = rig.paths()
        let tool = rig.tool(for: try rig.bundle("a"))
        try await tool.install()
        try await tool.remove()
        for file in files {
            #expect(
                try String(contentsOf: rig.home.appending(path: file), encoding: .utf8)
                    == "# mine\n")
        }
        // Only the folders under .local remain beyond what was there.
        let added = rig.paths().subtracting(before)
        #expect(added.allSatisfy { $0.hasPrefix(".local") })
    }

    @Test func noTemporaryLinkIsLeftBehind() async throws {
        let rig = try Home()
        let tool = rig.tool(for: try rig.bundle("a"))
        try await tool.install()
        try await tool.repair()
        #expect(rig.paths().contains { $0.contains(".stackly-link-") } == false)
    }

    @Test func aLinkToAnotherCopyIsStaleAndRepairPointsItHere() async throws {
        let rig = try Home()
        let first = try rig.bundle("a")
        let second = try rig.bundle("b")
        try await rig.tool(for: first).install()
        let moved = rig.tool(for: second)
        var status = await moved.status()
        #expect(status.command == .stale)
        #expect(status.manPages.allSatisfy { $0.state == .stale })
        try await moved.repair()
        status = await moved.status()
        #expect(status.command == .installed)
        #expect(status.commandTarget == second.appending(path: "Contents/MacOS/stackly-cli").path)
    }

    @Test func aLinkWhoseCopyIsGoneIsBrokenAndRepairFixesIt() async throws {
        let rig = try Home()
        let first = try rig.bundle("a")
        let second = try rig.bundle("b")
        try await rig.tool(for: first).install()
        try FileManager.default.removeItem(at: first.deletingLastPathComponent())
        let tool = rig.tool(for: second)
        var status = await tool.status()
        #expect(status.command == .broken)
        try await tool.repair()
        status = await tool.status()
        #expect(status.command == .installed)
    }

    @Test func aRegularFileIsAConflictThatNothingTouches() async throws {
        let rig = try Home()
        try FileManager.default.createDirectory(
            at: rig.commandLink.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("mine\n".utf8).write(to: rig.commandLink)
        let tool = rig.tool(for: try rig.bundle("a"))
        #expect(await tool.status().command == .conflict)
        let before = rig.paths()
        await #expect(throws: CommandLineToolError.conflict(path: "~/.local/bin/stackly")) {
            try await tool.install()
        }
        await #expect(throws: CommandLineToolError.self) { try await tool.repair() }
        try await tool.remove()
        #expect(try String(contentsOf: rig.commandLink, encoding: .utf8) == "mine\n")
        #expect(rig.paths() == before)
    }

    @Test func aForeignSymlinkIsAConflictToo() async throws {
        let rig = try Home()
        try FileManager.default.createDirectory(
            at: rig.commandLink.deletingLastPathComponent(), withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(
            atPath: rig.commandLink.path, withDestinationPath: "/usr/bin/true")
        let tool = rig.tool(for: try rig.bundle("a"))
        #expect(await tool.status().command == .conflict)
        await #expect(throws: CommandLineToolError.self) { try await tool.install() }
        try await tool.remove()
        #expect(
            try FileManager.default.destinationOfSymbolicLink(atPath: rig.commandLink.path)
                == "/usr/bin/true")
    }

    @Test func removeDeletesOnlyTheLinksStacklyMade() async throws {
        let rig = try Home()
        let tool = rig.tool(for: try rig.bundle("a"))
        try await tool.install()
        let foreign = rig.manFolder.appending(path: "stackly-other.1")
        try Data("mine\n".utf8).write(to: foreign)
        try await tool.remove()
        #expect(await tool.status().command == .notInstalled)
        #expect(FileManager.default.fileExists(atPath: foreign.path))
        #expect(FileManager.default.fileExists(atPath: rig.manFolder.path))
        #expect(
            FileManager.default.fileExists(atPath: rig.commandLink.deletingLastPathComponent().path)
        )
        #expect(
            (try? FileManager.default.destinationOfSymbolicLink(atPath: rig.commandLink.path))
                == nil)
    }

    @Test func aGroupWritableFolderIsRefusedBeforeAnythingIsWritten() async throws {
        let rig = try Home()
        let bin = rig.commandLink.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        try FileManager.default.setAttributes([.posixPermissions: 0o775], ofItemAtPath: bin.path)
        let tool = rig.tool(for: try rig.bundle("a"))
        let before = rig.paths()
        await #expect(throws: CommandLineToolError.folderNotSafe(path: "~/.local/bin")) {
            try await tool.install()
        }
        #expect(rig.paths() == before)
    }

    @Test func aBuildWithoutTheCommandCannotInstallButCanRemove() async throws {
        let rig = try Home()
        let full = try rig.bundle("a")
        try await rig.tool(for: full).install()
        let bare = rig.tool(for: try rig.bundle("b", withTool: false))
        #expect(bare.bundledTool == nil)
        await #expect(throws: CommandLineToolError.noBundledTool) { try await bare.install() }
        await #expect(throws: CommandLineToolError.noBundledTool) { try await bare.repair() }
        try await bare.remove()
        #expect(await bare.status().command == .notInstalled)
    }

    @Test func installRemovesAManagedPageLinkTheBundleNoLongerHas() async throws {
        let rig = try Home()
        try FileManager.default.createDirectory(
            at: rig.manFolder, withIntermediateDirectories: true)
        let old = rig.manFolder.appending(path: "stackly-old.1")
        try FileManager.default.createSymbolicLink(
            atPath: old.path,
            withDestinationPath: "/gone/Stackly.app/Contents/Resources/man/man1/stackly-old.1")
        let tool = rig.tool(for: try rig.bundle("a"))
        try await tool.install()
        #expect((try? FileManager.default.destinationOfSymbolicLink(atPath: old.path)) == nil)
        #expect(
            FileManager.default.fileExists(atPath: rig.manFolder.appending(path: "stackly.1").path))
    }

    @Test func aLinkThroughASymlinkedPathIsNotReportedStale() async throws {
        let rig = try Home()
        let real = try rig.bundle("a")
        let alias = rig.folder.url.appending(path: "alias")
        try FileManager.default.createSymbolicLink(
            at: alias, withDestinationURL: real.deletingLastPathComponent())
        // The tool is told the canonical bundle; the link points through the alias.
        try FileManager.default.createDirectory(
            at: rig.commandLink.deletingLastPathComponent(), withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(
            atPath: rig.commandLink.path,
            withDestinationPath: alias.appending(path: "Stackly.app/Contents/MacOS/stackly-cli")
                .path)
        #expect(await rig.tool(for: real).status().command == .installed)
    }
}
