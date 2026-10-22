import Foundation

/// The words of the Overview (spec 0022, *Copy*).
nonisolated public enum OverviewCopy {
    /// The subtitle of the page: "{ready} running · {stopped} stopped", then " · {failed}
    /// failed", " · {notReady} not ready", and " · {unknown} unknown" for each that is above
    /// zero.
    public static func subtitle(_ counts: InstanceCounts) -> String {
        var text = "\(counts.ready) running · \(counts.stopped) stopped"
        if counts.failed > 0 { text += " · \(counts.failed) failed" }
        if counts.notReady > 0 { text += " · \(counts.notReady) not ready" }
        if counts.unknown > 0 { text += " · \(counts.unknown) unknown" }
        return text
    }

    /// "Show 3 More".
    public static func showMore(_ count: Int) -> String { "Show \(count) More" }
    /// "Show Less".
    public static let showLess = "Show Less"
    /// "Show All 9 Instances".
    public static func showAllInstances(_ count: Int) -> String { "Show All \(count) Instances" }
    /// "Show All in Operations".
    public static let showAllOperations = "Show All in Operations"
    /// What an empty Needs Attention says.
    public static let nothingNeedsAttention = "Nothing needs attention."
    /// What an empty Operations section says.
    public static let nothingRunning = "Nothing running."

    /// The most items Needs Attention shows before "Show More".
    static let attentionLimit = 6
    /// The most instances the Instances section shows.
    static let instanceLimit = 6
    /// The most operations the Operations section shows.
    static let operationLimit = 5
}
