/// Turns the text the daemon sends into text for a screen.
nonisolated enum DaemonText {
    /// `text` with a capital first letter and a final period. The daemon writes a message
    /// in lower case without a period (spec 0006), so a screen shows it as a sentence.
    static func sentence(_ text: String) -> String {
        guard let first = text.first else { return text }
        let capitalized = first.uppercased() + text.dropFirst()
        return capitalized.hasSuffix(".") ? capitalized : capitalized + "."
    }
}
