import Foundation

/// The check that Increase Contrast changes a snapshot (spec 0068, AC-5, K15 gap G5): the
/// `hc-light` image must differ from `light`, and `hc-dark` from `dark`, unless the caller
/// says the view has nothing that reacts to Increase Contrast.
enum SnapshotContrast {
    /// The variant pairs: the high contrast variant and the standard one it must differ from.
    static let pairs: [(contrast: SnapshotVariant, standard: SnapshotVariant)] = [
        (.hcLight, .light), (.hcDark, .dark),
    ]

    /// The issue for a `contrastInvariant` flag without a usable reason, or `nil` when the
    /// flag is off or has one. Checked before anything renders.
    static func reasonIssue(name: String, invariant: Bool, reason: String?) -> String? {
        guard invariant, (reason?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "").isEmpty
        else { return nil }
        return "\(name): contrastInvariant needs a reason"
    }

    /// The issues for the images of one call.
    ///
    /// Only a pair whose two variants both rendered is checked, and the images are the
    /// ones just rendered, never the references on disk, so the check runs in record mode
    /// and before any reference exists. Identical means the same bytes, with no tolerance.
    ///
    /// - Parameters:
    ///   - name: The snapshot name.
    ///   - images: The rendered image of each variant.
    ///   - invariant: Whether the caller said the view does not react to Increase Contrast.
    /// - Returns: The messages of the issues, in pair order.
    static func issues(
        name: String, images: [SnapshotVariant: SnapshotImage], invariant: Bool
    ) -> [String] {
        var messages: [String] = []
        for (contrast, standard) in pairs {
            guard let high = images[contrast], let base = images[standard] else { continue }
            let identical = high == base
            if identical && invariant == false {
                messages.append(
                    "\(name) (\(contrast.suffix)): identical to \(standard.suffix); the view does "
                        + "not react to Increase Contrast. If that is intended, pass "
                        + "contrastInvariant: true with contrastInvariantReason")
            } else if identical == false && invariant {
                messages.append(
                    "\(name): contrastInvariant is set but \(contrast.suffix) differs from "
                        + "\(standard.suffix); remove it")
            }
        }
        return messages
    }
}
