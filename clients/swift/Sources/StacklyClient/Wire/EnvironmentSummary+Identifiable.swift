import Foundation

/// An environment is known by its ID, so the shell's environment store can keep it in a store
/// keyed by that ID (spec 0044, *Layar*).
extension EnvironmentSummary: Identifiable {}
