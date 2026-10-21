#if DEBUG
    import SwiftUI

    /// The gallery sections of milestone 2: sheet, checklist, staged progress.
    struct SheetComponentsSections: View {
        var body: some View {
            SheetGallerySection()
            CheckListGallerySection()
        }
    }

    // MARK: - Sheet

    /// The StacklySheet section: the states drawn inline, and a button that opens a real
    /// sheet.
    struct SheetGallerySection: View {
        @State private var isPresented = false

        var body: some View {
            GallerySection(
                "StacklySheet",
                note: "An opaque sheet with a header, a body, and a fixed footer. The footer "
                    + "writes why the main action cannot run. Return runs the main action "
                    + "unless it is destructive; Escape runs Cancel. The last button opens "
                    + "the real sheet."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s22) {
                    framed(
                        StacklySheet(
                            title: "New Instance", subtitle: ComponentSamples.sheetSubtitle,
                            tile: ComponentSamples.sheetTile,
                            primary: ComponentSamples.createAction,
                            minWidth: 560
                        ) { SheetSampleForm() })
                    framed(
                        StacklySheet(
                            title: "New Instance", subtitle: ComponentSamples.sheetSubtitle,
                            tile: ComponentSamples.sheetTile,
                            primary: ComponentSamples.createDisabledAction, minWidth: 560
                        ) { SheetSampleForm() })
                    framed(
                        StacklySheet(
                            title: "New Instance", subtitle: ComponentSamples.sheetSubtitle,
                            tile: ComponentSamples.sheetTile,
                            primary: ComponentSamples.createAction,
                            minWidth: 560
                        ) { SheetSampleForm() }
                    )
                    .environment(
                        \.mutationAvailability,
                        .blocked(reason: ComponentSamples.gateReason, since: .now)
                    )
                    framed(
                        StacklySheet(
                            title: "New Instance", subtitle: ComponentSamples.sheetSubtitle,
                            tile: ComponentSamples.sheetTile,
                            primary: ComponentSamples.createBusyAction, minWidth: 560
                        ) { SheetSampleForm() })
                    framed(
                        StacklySheet(
                            title: "Remove “pg17-dev”", primary: ComponentSamples.sheetRemoveAction,
                            minWidth: 560
                        ) {
                            Text(
                                "The instance is removed from Stackly. Its data directory is kept."
                            )
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        })
                    Button("Open Sheet") { isPresented = true }
                        .sheet(isPresented: $isPresented) {
                            StacklySheet(
                                title: "New Instance", subtitle: ComponentSamples.sheetSubtitle,
                                tile: ComponentSamples.sheetTile,
                                primary: StacklyAction(
                                    id: "sheet.create", title: "Create", mutates: true,
                                    perform: { isPresented = false }),
                                minWidth: 560
                            ) { SheetSampleForm() }
                        }
                }
            }
        }

        /// A sheet drawn inline: a fixed width and an outline, since there is no window.
        private func framed(_ sheet: some View) -> some View {
            sheet
                .frame(width: 560)
                .background(StacklyColor.surfaceWindow)
                .clipShape(RoundedRectangle(cornerRadius: Radius.large.value))
                .overlay {
                    RoundedRectangle(cornerRadius: Radius.large.value)
                        .strokeBorder(Color(nsColor: .separatorColor))
                }
        }
    }
#endif

#if DEBUG
    // MARK: - Checklist

    /// The CheckList section: every status, a row with an action, a long detail, and both
    /// layouts.
    struct CheckListGallerySection: View {
        var body: some View {
            GallerySection(
                "CheckList",
                note: "A symbol, a title, and the detail in full. The shape of the symbol "
                    + "differs per status, so color is never the only sign. The checking row "
                    + "shows its spinner after 300 ms. Adaptive is two columns in a sheet and "
                    + "one in a narrow width; single is always one."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s22) {
                    CardSection("Preflight · adaptive") {
                        CheckList(ComponentSamples.checkItems)
                            .padding(.vertical, Spacing.s4)
                    }
                    .frame(maxWidth: 560)
                    CardSection("Preflight · single") {
                        CheckList(
                            ComponentSamples.checkItems + [ComponentSamples.longDetailItem],
                            layout: .single
                        )
                        .padding(.vertical, Spacing.s4)
                    }
                    .frame(maxWidth: 560)
                }
                .actionHost()
            }
        }
    }
#endif
