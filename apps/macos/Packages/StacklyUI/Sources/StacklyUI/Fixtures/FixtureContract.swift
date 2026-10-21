#if DEBUG
    import Foundation
    import StacklyClient

    /// The contract values every fixture reports, fixed so a protocol bump never moves a
    /// scenario or a reference image (spec 0068).
    public nonisolated enum FixtureContract {
        /// The protocol version the fixture daemon and the fixture app both speak.
        public static let protocolVersion = ProtocolVersion(major: 1, minor: 8)

        /// The contract digest the fixture daemon reports.
        public static let digest =
            "2eccd79e67cade8b9797b1495437fd2a14a5e1e34a34cb63ba3062890aacc194"

        /// ``protocolVersion`` in `M.m` form, as the shell shows it.
        public static let protocolText = "\(protocolVersion.major).\(protocolVersion.minor)"
    }

    extension ConnectionDiagnostics {
        /// These diagnostics with the app's protocol replaced by the fixture's, since the
        /// live model reads it from the generated contract.
        public func pinningFixtureContract() -> ConnectionDiagnostics {
            var pinned = self
            pinned.appProtocol = FixtureContract.protocolText
            return pinned
        }
    }
#endif
