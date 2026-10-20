import Foundation

/// An operation is known by its ID, so the shell's operation center can keep it in a
/// store keyed by that ID (spec 0013, *Operation center*).
extension OperationSummary: Identifiable {}
