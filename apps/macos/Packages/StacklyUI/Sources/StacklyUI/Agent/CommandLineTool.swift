import Foundation

/// The live ``CommandLineToolControlling`` (spec 0015, *Instalasi CLI*): it links
/// `~/.local/bin/stackly` and the manual pages in `~/.local/share/man/man1` to the bundle
/// that runs. It is the only file of `StacklyUI` that creates, replaces, or deletes files
/// (a source rule keeps it so), and it never touches a shell file, `/etc/paths.d`, or
/// `/usr/local/bin`, and never asks for admin rights.
nonisolated final class CommandLineTool: CommandLineToolControlling {
    let home: URL
    private let bundle: URL

    /// Creates the tool for `bundle`, a canonical bundle path, with links under `home`.
    init(home: URL, bundle: URL) {
        self.home = home
        self.bundle = bundle
    }

    /// The tool for the running app: the home from the password database and the canonical
    /// path of the running bundle.
    static func live() -> CommandLineTool {
        CommandLineTool(
            home: UserHome.current(), bundle: CanonicalPath.resolve(Bundle.main.bundleURL))
    }

    // MARK: - Paths

    private var binFolder: URL { home.appending(path: ".local/bin") }
    private var manFolder: URL { home.appending(path: ".local/share/man/man1") }
    private var commandLink: URL { binFolder.appending(path: "stackly") }
    private var bundleManFolder: URL {
        bundle.appending(path: AgentIdentifiers.resourcesDir).appending(path: "man/man1")
    }

    var bundledTool: URL? {
        let url = bundle.appending(path: AgentIdentifiers.macosDir)
            .appending(path: AgentIdentifiers.cliExecutable)
        return FileManager.default.isExecutableFile(atPath: url.path) ? url : nil
    }

    /// The manual pages in the bundle, `stackly*.1`, sorted.
    private func bundleManPages() -> [String] {
        let names =
            (try? FileManager.default.contentsOfDirectory(atPath: bundleManFolder.path)) ?? []
        return names.filter { $0.hasPrefix("stackly") && $0.hasSuffix(".1") }.sorted()
    }

    // MARK: - Status

    @concurrent func status() async -> CommandLineToolStatus {
        let expected =
            bundledTool
            ?? bundle.appending(path: AgentIdentifiers.macosDir)
            .appending(path: AgentIdentifiers.cliExecutable)
        let command = state(
            of: commandLink, expectedTarget: expected, managed: Self.isManagedCommand)
        let pages = bundleManPages().map { name in
            CommandLineToolStatus.ManPage(
                name: name,
                state: state(
                    of: manFolder.appending(path: name),
                    expectedTarget: bundleManFolder.appending(path: name),
                    managed: Self.isManagedPage
                ).state)
        }
        return CommandLineToolStatus(
            command: command.state, commandTarget: command.target, manPages: pages)
    }

    /// Whether a link target is the command of some Stackly bundle.
    private static func isManagedCommand(_ target: String) -> Bool {
        target.hasSuffix(".app/\(AgentIdentifiers.macosDir)/\(AgentIdentifiers.cliExecutable)")
    }

    /// Whether a link target is a manual page of some Stackly bundle.
    private static func isManagedPage(_ target: String) -> Bool {
        let name = URL(filePath: target).lastPathComponent
        return target.contains(".app/\(AgentIdentifiers.resourcesDir)/man/man1/stackly")
            && name.hasPrefix("stackly") && name.hasSuffix(".1")
    }

    /// The state of the link at `path`, and its target when it is a link.
    private func state(
        of path: URL, expectedTarget: URL, managed: (String) -> Bool
    ) -> (state: CommandLineLinkState, target: String?) {
        let manager = FileManager.default
        guard let destination = try? manager.destinationOfSymbolicLink(atPath: path.path) else {
            // Not a link: nothing there, or something Stackly did not make.
            return (Self.exists(path) ? .conflict : .notInstalled, nil)
        }
        guard managed(destination) else { return (.conflict, destination) }
        let resolved = URL(
            fileURLWithPath: destination,
            relativeTo: path.deletingLastPathComponent()
        )
        guard manager.fileExists(atPath: resolved.path) else { return (.broken, destination) }
        let same =
            CanonicalPath.resolve(resolved).path == CanonicalPath.resolve(expectedTarget).path
        return (same ? .installed : .stale, destination)
    }

    /// Whether anything is at `path`, a dangling link included.
    private static func exists(_ path: URL) -> Bool {
        var info = stat()
        return lstat(path.path, &info) == 0
    }

    // MARK: - Install, repair, remove

    @concurrent func install() async throws(CommandLineToolError) {
        try link()
    }

    @concurrent func repair() async throws(CommandLineToolError) {
        try link()
    }

    private func link() throws(CommandLineToolError) {
        guard let tool = bundledTool else { throw .noBundledTool }
        let pages = bundleManPages()
        // Everything that can fail without writing fails first.
        try checkFolder(binFolder)
        try checkFolder(manFolder)
        let plan =
            [(commandLink, tool, Self.isManagedCommand)]
            + pages.map { name in
                (
                    manFolder.appending(path: name), bundleManFolder.appending(path: name),
                    Self.isManagedPage
                )
            }
        for (path, _, managed) in plan {
            let found = state(of: path, expectedTarget: path, managed: managed)
            if found.state == .conflict { throw .conflict(path: display(path)) }
        }
        try makeFolder(binFolder)
        try makeFolder(manFolder)
        for (path, target, _) in plan { try replace(path, with: target) }
        try removeStalePages(keeping: Set(pages))
    }

    @concurrent func remove() async throws(CommandLineToolError) {
        if Self.isLink(commandLink), Self.isManagedCommand(Self.target(of: commandLink)) {
            try delete(commandLink)
        }
        for name in Self.pageNames(in: manFolder) {
            let path = manFolder.appending(path: name)
            if Self.isLink(path), Self.isManagedPage(Self.target(of: path)) { try delete(path) }
        }
    }

    /// Deletes the managed manual page links that the bundle no longer has.
    private func removeStalePages(keeping pages: Set<String>) throws(CommandLineToolError) {
        for name in Self.pageNames(in: manFolder) where pages.contains(name) == false {
            let path = manFolder.appending(path: name)
            guard Self.isLink(path), Self.isManagedPage(Self.target(of: path)) else { continue }
            try delete(path)
        }
    }

    private static func pageNames(in folder: URL) -> [String] {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
        return names.filter { $0.hasPrefix("stackly") && $0.hasSuffix(".1") }.sorted()
    }

    private static func isLink(_ path: URL) -> Bool {
        (try? FileManager.default.destinationOfSymbolicLink(atPath: path.path)) != nil
    }

    private static func target(of path: URL) -> String {
        (try? FileManager.default.destinationOfSymbolicLink(atPath: path.path)) ?? ""
    }

    /// A folder that exists (or is a link to one) must belong to the user and not be writable
    /// by the group or others; a folder that does not exist is fine.
    private func checkFolder(_ folder: URL) throws(CommandLineToolError) {
        var info = stat()
        guard stat(folder.path, &info) == 0 else { return }
        let others = mode_t(S_IWGRP | S_IWOTH)
        guard info.st_uid == getuid(), info.st_mode & others == 0 else {
            throw .folderNotSafe(path: display(folder))
        }
    }

    private func makeFolder(_ folder: URL) throws(CommandLineToolError) {
        do {
            try FileManager.default.createDirectory(
                at: folder, withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o755])
        } catch {
            throw .io(path: display(folder), reason: error.localizedDescription)
        }
    }

    /// Makes `path` a link to `target` in one step: the new link is written beside it under
    /// a temporary name, then renamed over it.
    private func replace(_ path: URL, with target: URL) throws(CommandLineToolError) {
        let temporary = path.deletingLastPathComponent()
            .appending(path: ".stackly-link-\(UUID().uuidString)")
        do {
            try FileManager.default.createSymbolicLink(
                atPath: temporary.path, withDestinationPath: target.path)
        } catch {
            throw .io(path: display(path), reason: error.localizedDescription)
        }
        guard rename(temporary.path, path.path) == 0 else {
            let reason = String(cString: strerror(errno))
            try? FileManager.default.removeItem(at: temporary)
            throw .io(path: display(path), reason: reason)
        }
    }

    private func delete(_ path: URL) throws(CommandLineToolError) {
        do {
            try FileManager.default.removeItem(at: path)
        } catch {
            throw .io(path: display(path), reason: error.localizedDescription)
        }
    }

    private func display(_ path: URL) -> String {
        UserHome.tildePath(path.path, home: home.path)
    }
}
