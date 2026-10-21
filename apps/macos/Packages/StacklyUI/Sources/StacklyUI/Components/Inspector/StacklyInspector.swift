import SwiftUI

/// The measures of the inspector column (spec 0067, AC-18, and spec 0022): 240 pt at the
/// least, 280 pt by default, and 360 pt at the most.
nonisolated enum InspectorMetrics {
    /// The narrowest the column can be dragged to.
    static let minWidth: CGFloat = 240
    /// The width the column opens at.
    static let idealWidth: CGFloat = 280
    /// The widest the column can be dragged to.
    static let maxWidth: CGFloat = 360
    /// The space between the column's edge and its content.
    static let padding = Spacing.s16
}

extension View {
    /// Shows a native inspector column on the right of this view (spec 0067, AC-18) and
    /// puts an ``InspectorToggle`` in the toolbar.
    ///
    /// The column is 240 pt wide at the least, 280 pt by default, and 360 pt at the most.
    /// Its content scrolls on the opaque ``StacklyColor/surfaceWindow``, with 16 pt of
    /// padding and 16 pt between the views. Put it on the page content of the detail
    /// column, the content that fills the column: the inspector's container takes the
    /// height it is offered and has none of its own, so on a view inside a `ScrollView` its
    /// content is drawn over the neighbouring views. The screen keeps the binding in `@SceneStorage("<page>.inspector")` with
    /// the inspector open by default (`apps/macos/design.md`, Components, Inspector). It
    /// is the only place that calls `.inspector(`.
    ///
    /// Build the content from ``CardSection``, ``InspectorHeader``, ``InspectorPlaceholder``,
    /// ``StateBanner``, and ``ActionButton``.
    ///
    /// - Parameters:
    ///   - isPresented: Whether the inspector is open.
    ///   - help: The help of the toolbar toggle, for example with the shortcut; `nil` uses
    ///     the toggle's label.
    ///   - content: The inspector's content.
    public func stacklyInspector<C: View>(
        isPresented: Binding<Bool>,
        help: String? = nil,
        @ViewBuilder content: () -> C
    ) -> some View {
        modifier(
            StacklyInspectorModifier(isPresented: isPresented, help: help, inspector: content())
        )
    }
}

/// The modifier behind ``SwiftUI/View/stacklyInspector(isPresented:help:content:)``.
private struct StacklyInspectorModifier<Inspector: View>: ViewModifier {
    @Binding var isPresented: Bool
    let help: String?
    let inspector: Inspector

    @State private var viewportHeight: CGFloat = 0

    func body(content: Content) -> some View {
        content
            .inspector(isPresented: $isPresented) {
                ScrollView {
                    VStack(alignment: .leading, spacing: Spacing.s16) {
                        inspector
                    }
                    .frame(
                        maxWidth: .infinity,
                        minHeight: max(0, viewportHeight - 2 * InspectorMetrics.padding),
                        alignment: .top
                    )
                    .padding(InspectorMetrics.padding)
                }
                .onGeometryChange(for: CGFloat.self) {
                    $0.size.height
                } action: {
                    viewportHeight = $0
                }
                .background(StacklyColor.surfaceWindow)
                .inspectorColumnWidth(
                    min: InspectorMetrics.minWidth,
                    ideal: InspectorMetrics.idealWidth,
                    max: InspectorMetrics.maxWidth
                )
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    InspectorToggle(isPresented: $isPresented, help: help)
                }
            }
    }
}
