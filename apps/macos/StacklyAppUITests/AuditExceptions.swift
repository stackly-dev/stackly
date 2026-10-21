import XCTest

/// An accessibility issue that a spec accepted (spec 0068, AC-17).
struct AuditException {
    /// The kind of audit that reports the issue.
    let type: XCUIAccessibilityAuditType
    /// The identifier of the element, or text its label contains.
    let element: String
    /// Why the issue stays.
    let reason: String
    /// The spec that accepted it, such as "spec 0012".
    let acceptedBy: String
}

/// The accepted audit issues. The list starts empty, and a row is added only after the lead
/// approved it: the element, the kind, the reason, and the spec that accepts it.
enum AuditExceptions {
    static let all: [AuditException] = []

    /// Whether an exception covers `issue`: the same kind, and an element whose identifier
    /// equals the row's element or whose label contains it.
    static func matches(_ issue: XCUIAccessibilityAuditIssue) -> Bool {
        all.contains { exception in
            guard issue.auditType == exception.type, let element = issue.element else {
                return false
            }
            return element.identifier == exception.element
                || element.label.contains(exception.element)
        }
    }
}
