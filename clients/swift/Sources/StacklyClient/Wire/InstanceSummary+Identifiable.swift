import Foundation

/// An instance is known by its ID, so the shell's instance store can keep it in a store
/// keyed by that ID (spec 0013, *Daftar instance sementara*).
extension InstanceSummary: Identifiable {}
