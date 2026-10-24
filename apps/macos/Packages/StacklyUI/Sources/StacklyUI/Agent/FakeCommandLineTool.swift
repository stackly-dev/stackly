#if DEBUG
    import Foundation
    import Synchronization

    /// An in-memory ``CommandLineToolControlling`` for tests, previews, and fixtures: it
    /// writes no file and reads no home (spec 0015, *Banner, pengumuman, dan environment*).
    /// `install()` and `repair()` make the command and every manual page `installed`;
    /// `remove()` makes them `not_installed`. An error a test queued is thrown once, and
    /// the status stays.
    nonisolated final class FakeCommandLineTool: CommandLineToolControlling {
        /// One call the fake received.
        enum Call: Sendable, Equatable { case status, install, repair, remove }

        /// The home of the fixtures of spec 0068.
        static let fixtureHome = URL(filePath: "/fixture/home")
        /// The command of the fixtures of spec 0068, inside the fixture bundle.
        static let fixtureTool = URL(
            filePath: "/fixture/Applications/Stackly.app/Contents/MacOS/stackly-cli")
        /// The status of a fresh install in the fixtures: nothing linked, one manual page.
        static let fixtureStatus = CommandLineToolStatus(
            command: .notInstalled, manPages: [.init(name: "stackly.1", state: .notInstalled)])

        let home: URL
        let bundledTool: URL?

        private struct State {
            var status: CommandLineToolStatus
            var calls: [Call] = []
            var errors: [CommandLineToolError] = []
        }

        private let state: Mutex<State>

        /// Creates the fake.
        /// - Parameters:
        ///   - home: The home the model writes paths against.
        ///   - bundledTool: The command in the bundle, or `nil` for a build without it.
        ///   - status: The status at first.
        init(
            home: URL = FakeCommandLineTool.fixtureHome,
            bundledTool: URL? = FakeCommandLineTool.fixtureTool,
            status: CommandLineToolStatus = FakeCommandLineTool.fixtureStatus
        ) {
            self.home = home
            self.bundledTool = bundledTool
            state = Mutex(State(status: status))
        }

        /// The calls so far, in order.
        var calls: [Call] { state.withLock { $0.calls } }

        /// Replaces the status.
        func setStatus(_ status: CommandLineToolStatus) {
            state.withLock { $0.status = status }
        }

        /// Queues an error for the next install, repair, or remove.
        func failNext(_ error: CommandLineToolError) {
            state.withLock { $0.errors.append(error) }
        }

        @concurrent func status() async -> CommandLineToolStatus {
            state.withLock { state in
                state.calls.append(.status)
                return state.status
            }
        }

        @concurrent func install() async throws(CommandLineToolError) {
            try change(.install, to: .installed)
        }

        @concurrent func repair() async throws(CommandLineToolError) {
            try change(.repair, to: .installed)
        }

        @concurrent func remove() async throws(CommandLineToolError) {
            try change(.remove, to: .notInstalled)
        }

        private func change(_ call: Call, to link: CommandLineLinkState)
            throws(CommandLineToolError)
        {
            let error = state.withLock { state -> CommandLineToolError? in
                state.calls.append(call)
                guard state.errors.isEmpty else { return state.errors.removeFirst() }
                state.status.command = link
                state.status.commandTarget =
                    link == .installed ? FakeCommandLineTool.fixtureTool.path : nil
                state.status.manPages = state.status.manPages.map {
                    .init(name: $0.name, state: link)
                }
                return nil
            }
            if let error { throw error }
        }
    }
#endif
