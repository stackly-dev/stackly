/// The description of a console block’s optional Copy button.
nonisolated public struct ConsoleCopy: Equatable, Sendable {
    /// What the text is, in lower case: "example", "details". Makes the label "Copy example".
    public var what: String

    /// Creates the copy description.
    /// - Parameters:
    ///   - what: What the text is, in lower case.
    public init(what: String) {
        self.what = what

    }
}
