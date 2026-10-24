import Foundation

/// The instance a duplicate copies the settings of (spec 0024, *Sheet mode duplicate*).
nonisolated public struct DuplicateSourceRef: Hashable, Sendable, Identifiable {
    /// The ID of the source instance.
    public let id: UUID
    /// The name of the source instance, as the sheet and the feedback say it.
    public let name: String

    /// Creates a reference.
    /// - Parameters:
    ///   - id: The ID of the source instance.
    ///   - name: Its name.
    public init(id: UUID, name: String) {
        self.id = id
        self.name = name
    }
}

/// How the New Instance sheet starts (spec 0024, *Sheet mode duplicate*).
nonisolated public enum CreateMode: Hashable, Sendable {
    /// A new instance of a runtime the person chooses, on the software the Catalog asked for
    /// or the first runtime (spec 0020).
    case create(software: String?)
    /// A new instance of the runtime of `source`, with its settings filled in.
    case duplicate(DuplicateSourceRef)
}
