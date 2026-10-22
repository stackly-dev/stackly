#if DEBUG
    import Foundation

    /// The one clock fixtures and snapshots use, so a rendered time never moves between
    /// runs (spec 0068).
    public nonisolated enum FixtureClock {
        /// The fixed "now" of every fixture; the value `ShellPreviewSamples` already used.
        public static let now = Date(timeIntervalSince1970: 1_792_300_000)

        /// The moment `seconds` seconds before ``now``; the 15 second limit of the agent
        /// banner uses it.
        public static func secondsAgo(_ seconds: Int) -> Date {
            now.addingTimeInterval(-Double(seconds))
        }

        /// The moment `minutes` minutes before ``now``.
        public static func minutesAgo(_ minutes: Int) -> Date {
            now.addingTimeInterval(-Double(minutes) * 60)
        }

        /// The moment `days` days before ``now``.
        public static func daysAgo(_ days: Int) -> Date {
            now.addingTimeInterval(-Double(days) * 86_400)
        }
    }
#endif
