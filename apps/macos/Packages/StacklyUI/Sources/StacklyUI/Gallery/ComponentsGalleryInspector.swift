#if DEBUG
    import SwiftUI

    /// The gallery sections of milestone 3: the inspector.
    struct InspectorComponentsSections: View {
        var body: some View {
            InspectorHeaderGallerySection()
            InspectorPlaceholderGallerySection()
            InspectorLiveGallerySection()
        }
    }

    // MARK: - Header

    /// A sample inspector column: the opaque window surface at a fixed width, with the
    /// padding and spacing `.stacklyInspector` gives its content.
    private struct InspectorColumnSample<Content: View>: View {
        let width: CGFloat
        @ViewBuilder let content: Content

        var body: some View {
            VStack(alignment: .leading, spacing: Spacing.s16) {
                content
            }
            .padding(InspectorMetrics.padding)
            .frame(width: width, alignment: .top)
            .background(StacklyColor.surfaceWindow)
            .overlay {
                Rectangle().strokeBorder(Color(nsColor: .separatorColor))
            }
        }
    }

    /// The InspectorHeader section: with a tile, status, and actions; without actions; and
    /// with a title that wraps at the narrowest column; then a card in a column.
    struct InspectorHeaderGallerySection: View {
        var body: some View {
            GallerySection(
                "InspectorHeader",
                note: "Centered in the column: the 40 pt tile, the title, the subtitle, the "
                    + "status, and the actions. Texts wrap and are never cut; the actions "
                    + "stack when they do not fit side by side. The columns here are 280 pt, "
                    + "the default, and 240 pt, the narrowest."
            ) {
                ScrollView(.horizontal) {
                    HStack(alignment: .top, spacing: Spacing.s14) {
                        InspectorColumnSample(width: ComponentSamples.inspectorColumnWidth) {
                            InspectorHeader(
                                title: ComponentSamples.inspectorTitle,
                                subtitle: ComponentSamples.inspectorSubtitle,
                                tile: ComponentSamples.inspectorTile,
                                status: ComponentSamples.inspectorStatus
                            ) {
                                ActionButton(ComponentSamples.stopAction)
                                ActionButton(ComponentSamples.removeAction)
                            }
                            CardSection("Summary") {
                                ForEach(ComponentSamples.summaryRows) { row in
                                    LabeledContent(row.label) {
                                        if row.isCode {
                                            Text(row.value).font(.callout.monospaced())
                                        } else {
                                            Text(row.value)
                                        }
                                    }
                                }
                            }
                            .labeledContentStyle(.stackly)
                        }
                        InspectorColumnSample(width: ComponentSamples.inspectorColumnWidth) {
                            InspectorHeader(
                                title: ComponentSamples.inspectorTitle,
                                subtitle: ComponentSamples.inspectorSubtitle,
                                tile: ComponentSamples.inspectorTile,
                                status: ComponentSamples.inspectorStoppedStatus
                            )
                        }
                        InspectorColumnSample(width: ComponentSamples.inspectorNarrowColumnWidth) {
                            InspectorHeader(
                                title: ComponentSamples.inspectorLongTitle,
                                subtitle: ComponentSamples.inspectorLongSubtitle,
                                tile: ComponentSamples.inspectorTile,
                                status: ComponentSamples.inspectorStatus
                            ) {
                                ActionButton(ComponentSamples.startAction, prominence: .primary)
                                ActionButton(ComponentSamples.copyAction)
                            }
                        }
                    }
                }
            }
            .actionHost()
        }
    }

    // MARK: - Placeholder

    /// The InspectorPlaceholder section: the default title with a message, and a custom title.
    struct InspectorPlaceholderGallerySection: View {
        var body: some View {
            GallerySection(
                "InspectorPlaceholder",
                note: "What the inspector says when nothing is selected. One VoiceOver group: "
                    + "the title, then the message."
            ) {
                HStack(alignment: .top, spacing: Spacing.s14) {
                    InspectorColumnSample(width: ComponentSamples.inspectorColumnWidth) {
                        InspectorPlaceholder(message: ComponentSamples.inspectorPlaceholderMessage)
                            .frame(height: 160)
                    }
                    InspectorColumnSample(width: ComponentSamples.inspectorColumnWidth) {
                        InspectorPlaceholder(title: "Nothing to Show")
                            .frame(height: 160)
                    }
                }
            }
        }
    }

    // MARK: - Live inspector

    /// The `.stacklyInspector` section: a button that opens a window-sized page with a real
    /// inspector and its toggle.
    struct InspectorLiveGallerySection: View {
        @State private var showsDemo = false

        var body: some View {
            GallerySection(
                "stacklyInspector and InspectorToggle",
                note: "The native inspector column, 240 to 360 pt, on the opaque window "
                    + "surface. An inspector belongs to a whole window, not to a view inside "
                    + "a scrolling page, so the demo opens its own page in a sheet, with the "
                    + "toggle in the page and the same toggle that a screen puts in its toolbar."
            ) {
                Button("Open Inspector Demo") {
                    showsDemo = true
                }
            }
            .sheet(isPresented: $showsDemo) {
                InspectorDemoPage()
            }
        }
    }

    /// A page with a real `.stacklyInspector`, shown in a sheet by the gallery.
    private struct InspectorDemoPage: View {
        @Environment(\.dismiss) private var dismiss
        @State private var isPresented = true

        var body: some View {
            VStack(alignment: .leading, spacing: Spacing.s12) {
                HStack(spacing: Spacing.s12) {
                    InspectorToggle(isPresented: $isPresented)
                    Text(InspectorToggle.label(isPresented: isPresented))
                        .foregroundStyle(StacklyColor.textSecondary)
                    Spacer()
                    Button("Close") { dismiss() }
                        .keyboardShortcut(.cancelAction)
                }
                Text(
                    "The page content fills the rest of the window; the inspector is on its right."
                )
                .foregroundStyle(StacklyColor.textSecondary)
                Spacer()
            }
            .padding(Spacing.s22)
            .frame(minWidth: 720, minHeight: 380, alignment: .topLeading)
            .background(StacklyColor.surfaceWindow)
            .stacklyInspector(isPresented: $isPresented) {
                InspectorHeader(
                    title: ComponentSamples.inspectorTitle,
                    subtitle: ComponentSamples.inspectorSubtitle,
                    tile: ComponentSamples.inspectorTile,
                    status: ComponentSamples.inspectorStatus
                )
                CardSection("Summary") {
                    ForEach(ComponentSamples.summaryRows) { row in
                        LabeledContent(row.label, value: row.value)
                    }
                }
                .labeledContentStyle(.stackly)
            }
        }
    }
#endif
