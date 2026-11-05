#if DEBUG
    import Foundation

    /// What the Open In menus ask of the system, answered from data (spec 0068 as extended by
    /// spec 0070): which applications are installed, and a recorder of what was opened.
    /// Previews, snapshots, and tests share it, so no snapshot depends on the applications of
    /// the machine it renders on.
    @MainActor
    final class FixtureOpenIn {
        /// The bundle IDs the fake machine has installed: TablePlus and Terminal, not Postico,
        /// so the Postico entry is hidden (AC-5).
        static let installed: Set<String> = ["com.tinyapp.TablePlus", OpenInModel.terminalBundleID]

        /// The folder the fake applications live in.
        static let applications = URL(fileURLWithPath: "/Applications")

        /// A URL opened with an application, and which one.
        struct Opened: Equatable {
            /// What was opened.
            let url: URL
            /// The application that opened it.
            let application: URL
        }

        /// The bundle IDs the locator knows.
        var bundles: Set<String>
        /// The paths `reveal` finds.
        var existingPaths: Set<String> = []
        /// The paths that are not a regular file (a symlink, a folder, or nothing); every other
        /// path is one, so a fixture terminal script opens.
        var notRegularFiles: Set<String> = []
        /// What `open(_:withApplicationAt:)` throws, or `nil`.
        var openFailure: (any Error)?
        /// What the browser open throws, or `nil`.
        var browserFailure: (any Error)?
        /// The URLs opened with an application, in order.
        private(set) var opened: [Opened] = []
        /// The URLs opened in the default browser, in order.
        private(set) var browsed: [URL] = []
        /// The files shown in Finder, in order.
        private(set) var revealed: [URL] = []

        /// Creates the fakes with `bundles` installed.
        init(bundles: Set<String> = FixtureOpenIn.installed) {
            self.bundles = bundles
        }

        /// The ports over these fakes.
        var ports: OpenInPorts {
            OpenInPorts(
                locator: Locator(owner: self), opener: Opener(owner: self),
                fileExists: { [weak self] in self?.existingPaths.contains($0) ?? false },
                isRegularFile: { [weak self] path in
                    self.map { $0.notRegularFiles.contains(path) == false } ?? false
                })
        }

        fileprivate func record(_ url: URL, with application: URL) throws {
            if let openFailure { throw openFailure }
            opened.append(Opened(url: url, application: application))
        }

        fileprivate func record(browsing url: URL) throws {
            if let browserFailure { throw browserFailure }
            browsed.append(url)
        }

        fileprivate func record(revealing file: URL) {
            revealed.append(file)
        }

        private struct Locator: AppLocator {
            let owner: FixtureOpenIn

            func applicationURL(forBundleIdentifier bundleIdentifier: String) -> URL? {
                owner.bundles.contains(bundleIdentifier)
                    ? FixtureOpenIn.applications.appending(path: "\(bundleIdentifier).app") : nil
            }
        }

        private struct Opener: WorkspaceOpener {
            let owner: FixtureOpenIn

            func open(_ url: URL, withApplicationAt application: URL) async throws {
                try owner.record(url, with: application)
            }

            func openInBrowser(_ url: URL) throws {
                try owner.record(browsing: url)
            }

            func reveal(_ file: URL) {
                owner.record(revealing: file)
            }
        }
    }
#endif
