import SwiftUI

/// The window banner of a daemon restart (spec 0030 AC-16): a ``StateBanner`` on the window
/// surface that takes the place of the daemon banner while the daemon is away or coming back,
/// and warns when the restart did not go to plan. It draws the ``RestartBannerContent`` the
/// model derives; its only action is Dismiss, offered once the plan ended.
public struct RestartBanner: View {
    private let content: RestartBannerContent
    private let dismiss: @MainActor () -> Void

    /// Creates the banner.
    /// - Parameters:
    ///   - content: What the model derived from the plan.
    ///   - dismiss: Hides the banner of the plan that ended.
    public init(_ content: RestartBannerContent, dismiss: @escaping @MainActor () -> Void) {
        self.content = content
        self.dismiss = dismiss
    }

    /// The banner's content.
    public var body: some View {
        StateBanner(
            tone: content.tone,
            title: content.title,
            message: content.message,
            items: content.items.map { PartialItem(name: $0.name, reason: $0.reason) },
            actions: content.canDismiss
                ? [StateAction(title: "Dismiss", mutates: false, perform: dismiss)] : [],
            base: .window
        )
    }
}
