import Testing

/// The parent of every snapshot suite, so snapshots never render at the same time (spec
/// 0068). `.serialized` applies to the suites nested in it through `extension
/// SnapshotTests`.
@Suite(.serialized)
struct SnapshotTests {}
