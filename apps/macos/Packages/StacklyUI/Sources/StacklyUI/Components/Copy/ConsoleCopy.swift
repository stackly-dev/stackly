/// What a ``ConsoleBlock`` of plain text needs to offer a Copy button: what it is called
/// and what VoiceOver says after the copy.
nonisolated public struct ConsoleCopy: Equatable, Sendable {
    /// What the text is, in lower case: "example", "details". Makes the label "Copy example".
    public var what: String
    /// What VoiceOver says after the copy, or `nil` for "{What} copied."
    public var announcement: String?

    /// Creates the copy description.
    ///
    /// - Parameters:
    ///   - what: What the text is, in lower case.
    ///   - announcement: What VoiceOver says after the copy, or `nil` for the default.
    public init(what: String, announcement: String? = nil) {
        self.what = what
        self.announcement = announcement
    }
}
