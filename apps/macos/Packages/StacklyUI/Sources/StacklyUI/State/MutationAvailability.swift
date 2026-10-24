import SwiftUI

/// Whether actions that change data through the daemon can run right now.
/// The app shell sets it in the environment (`.environment(\.mutationAvailability, …)`),
/// for example to `.blocked` while the daemon cannot be reached, and shows the one window
/// banner that explains why. Views read it through ``SwiftUI/View/mutationGated()`` and
/// through ``StateAction`` values with `mutates` set.
nonisolated public enum MutationAvailability: Hashable, Sendable {
    /// Mutations can run.
    case available
    /// Mutations cannot run, for `reason`, since `since`.
    case blocked(reason: String, since: Date)

    /// The reason mutations are blocked, or `nil` while they are available.
    public var blockedReason: String? {
        switch self {
        case .available: nil
        case .blocked(let reason, _): reason
        }
    }
}

extension EnvironmentValues {
    /// Whether actions that change data can run. The app shell sets it; the default is
    /// `.available`.
    @Entry public var mutationAvailability: MutationAvailability = .available
}

extension View {
    /// Apply it to every control that changes data through the daemon. Never apply it to
    /// a control that does not: copy, navigation, Retry, and Open Diagnostics stay enabled.
    /// The visible reason is the app shell's window banner.
    public func mutationGated() -> some View {
        modifier(MutationGate())
    }

    /// Use it for a capability that is not supported.
    /// The help alone is never enough: put ``DisabledReasonText`` with the same reason
    /// next to the control, in the section footer, or use an unsupported placeholder. Never
    /// hide the control instead.
    /// Apply it to the control itself, not to a container: the help is added only while
    /// there is a reason, so the control keeps its own help while it is enabled, and a
    /// change of reason re-creates the control.
    /// - Parameter reason: Why the view is disabled, or `nil` to leave it enabled.
    public func disabled(reason: String?) -> some View {
        modifier(DisabledReasonModifier(reason: reason))
    }
}

private struct DisabledReasonModifier: ViewModifier {
    let reason: String?

    func body(content: Content) -> some View {
        // No `.help("")` while enabled: an empty help on the outside could hide the
        // control's own help.
        if let reason {
            content
                .disabled(true)
                .help(reason)
        } else {
            content
        }
    }
}

private struct MutationGate: ViewModifier {
    @Environment(\.mutationAvailability) private var availability

    func body(content: Content) -> some View {
        content.disabled(reason: availability.blockedReason)
    }
}

/// The visible reason a control is disabled: an `info.circle` symbol and the reason in
/// `.footnote` ``StacklyColor/textSecondary``.
/// Put it next to a control disabled with ``SwiftUI/View/disabled(reason:)``, or in the
/// section footer, so the reason never lives only in a tooltip.
public struct DisabledReasonText: View {
    private let reason: String

    /// Creates the reason text.
    /// - Parameter reason: Why the control is disabled.
    public init(_ reason: String) {
        self.reason = reason
    }

    /// The view's content.
    public var body: some View {
        Label {
            Text(reason)
                .fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: "info.circle")
        }
        .font(.footnote)
        .foregroundStyle(StacklyColor.textSecondary)
    }
}
