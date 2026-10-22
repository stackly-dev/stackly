import Foundation

/// A runtime is known by its ID, so the shell's runtime store can keep it in a store keyed
/// by that ID (spec 0018, *Halaman Runtimes*).
extension RuntimeSummary: Identifiable {}

/// A running install is known by its operation, the one thing that tells two installs of
/// the same version apart while they run (spec 0018, *Halaman Runtimes*).
extension InstallingRuntime: Identifiable {
    /// The `install_runtime` operation.
    public var id: UUID { operationId }
}
