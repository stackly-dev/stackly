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

/// The widest the inspector column can be for a window width (spec 0067, AC-18).
/// At the smallest window (900 pt) with the sidebar (236 pt) and the inspector open, the
/// page column is 384 pt wide when the inspector is 280 pt. Below 1000 pt the column is
/// therefore capped at 280 pt, so 384 pt stays the narrowest the page column gets; from
/// 1000 pt up it can be dragged to 360 pt.
nonisolated public enum StacklyInspectorWidth {
    /// The window width below which the inspector is capped at its ideal width.
    public static let compactWindowWidth: CGFloat = 1000

    /// The widest the inspector column can be at `windowWidth`.
    /// - Parameter windowWidth: The width of the window, or `nil` when it is not known
    ///   (gallery, previews, sheets), which allows the full width.
    /// - Returns: 280 pt below ``compactWindowWidth``, otherwise 360 pt.
    public static func maxWidth(windowWidth: CGFloat?) -> CGFloat {
        guard let windowWidth, windowWidth < compactWindowWidth else {
            return InspectorMetrics.maxWidth
        }
        return InspectorMetrics.idealWidth
    }
}

extension EnvironmentValues {
    /// The width of the window the view is in. The root view of the window sets it from
    /// its geometry; `nil` (the default) means the window is not known and the inspector
    /// may be 360 pt wide.
    @Entry public var stacklyWindowWidth: CGFloat?
}

extension View {
    /// Shows a native inspector column on the right of this view (spec 0067, AC-18) and
    /// puts an ``InspectorToggle`` in the toolbar.
    /// The column is 240 pt wide at the least, 280 pt by default, and 360 pt at the most
    /// in a window 1000 pt wide or more, 280 pt at the most in a narrower one
    /// (``StacklyInspectorWidth``, from the window width in the environment).
    /// Its content scrolls on the opaque ``StacklyColor/surfaceWindow``, with 16 pt of
    /// padding and 16 pt between the views. Put it on the page content of the detail
    /// column, the content that fills the column: the inspector's container takes the
    /// height it is offered and has none of its own, so on a view inside a `ScrollView` its
    /// content is drawn over the neighbouring views. The screen keeps the binding in `@SceneStorage("<page>.inspector")` with
    /// the inspector open by default (`apps/macos/design.md`, Components, Inspector). It
    /// is the only place that calls `.inspector(`.
    /// Build the content from ``CardSection``, ``InspectorHeader``, ``InspectorPlaceholder``,
    /// ``StateBanner``, and ``ActionButton``.
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

    @Environment(\.stacklyWindowWidth) private var windowWidth
    @State private var viewportHeight: CGFloat = 0

    func body(content: Content) -> some View {
        content

            // The split view of `.inspector` measures the page at 0 × 0, where wrapping
            // text is thousands of points tall, and lays the page out at that height. The
            // page takes the size the column offers instead, open or closed.
            .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
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
                    // The system holds the column in Liquid Glass and gives the content the
                    // glass material, so SwiftUI would draw its controls, symbols, and text
                    // vibrant. The column is opaque, so the content draws as on any opaque
                    // surface, in the token colors. Like the relay, it reaches the content only
                    // here, inside the `ScrollView`.
                    .environment(\.backgroundMaterial, nil)

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
                    max: StacklyInspectorWidth.maxWidth(windowWidth: windowWidth)
                )
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    InspectorToggle(isPresented: $isPresented, help: help)
                }
            }
    }
}
