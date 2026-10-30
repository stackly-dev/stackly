#if DEBUG
    import SwiftUI

    /// The gallery sections of milestone 2: sheet, checklist, staged progress.
    struct SheetComponentsSections: View {
        var body: some View {
            SheetGallerySection()
            CheckListGallerySection()
            StagedProgressGallerySection()
        }
    }

    // MARK: - Sheet

    /// The StacklySheet section: the states drawn inline, and a button that opens a real
    /// sheet.
    struct SheetGallerySection: View {
        @State private var isPresented = false
        @State private var isRemovePresented = false
        @State private var isLongPresented = false

        var body: some View {
            GallerySection(
                "StacklySheet",
                note: "An opaque sheet with a header, a body, and a fixed footer. The footer "
                    + "writes why the main action cannot run. The last three buttons open "
                    + "real sheets; the long one scrolls its body under a fixed header and "
                    + "footer."
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
                        .blocked(reason: ComponentSamples.gateReason, since: FixtureClock.now)
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
                    HStack(spacing: Spacing.s12) {
                        Button("Open Sheet") { isPresented = true }
                        Button("Open Remove Sheet") { isRemovePresented = true }
                        Button("Open Long Sheet") { isLongPresented = true }
                    }
                    .sheet(isPresented: $isLongPresented) {
                        StacklySheet(
                            title: "New Instance", subtitle: ComponentSamples.sheetSubtitle,
                            tile: ComponentSamples.sheetTile,
                            primary: StacklyAction(
                                id: "sheet.create", title: "Create", mutates: true,
                                perform: { isLongPresented = false }),
                            minWidth: 560
                        ) { SheetSampleLongForm() }
                    }
                    .sheet(isPresented: $isRemovePresented) {
                        StacklySheet(
                            title: "Remove “pg17-dev”",
                            primary: StacklyAction(
                                id: "sheet.remove", title: "Remove…",
                                role: .destructive(ComponentSamples.removeConfirmation),
                                mutates: true, perform: { isRemovePresented = false }),
                            minWidth: 560
                        ) {
                            Text(
                                "Remove asks again, on top of this sheet."
                            )
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
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

    /// Widths the gallery samples are drawn at.
    enum ComponentsGalleryWidth {
        /// A width inside the inspector's range (280 to 360 pt), where adaptive layouts
        /// turn to one column and stages wrap.
        static let inspector: CGFloat = 320
    }

    /// The CheckList section: every status, a row with an action, a long detail, and both
    /// layouts.
    struct CheckListGallerySection: View {
        var body: some View {
            GallerySection(
                "CheckList",
                note: "A symbol, a title, and the detail in full. The shape of the symbol "
                    + "differs per status, so color is never the only sign. The checking row "
                    + "shows its spinner after 300 ms. Adaptive is two columns in a sheet and "
                    + "one in an inspector; single is always one."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s22) {
                    CardSection("Preflight · adaptive") {
                        CheckList(ComponentSamples.checkItems)
                            .padding(.vertical, Spacing.s4)
                    }
                    .frame(maxWidth: 560)
                    CardSection("Preflight · adaptive, inspector width") {
                        CheckList(ComponentSamples.checkItems)
                            .padding(.vertical, Spacing.s4)
                    }
                    .frame(maxWidth: ComponentsGalleryWidth.inspector)
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

#if DEBUG
    // MARK: - Staged progress

    /// The StagedProgress section: the compact line and the stages layout in every outcome.
    struct StagedProgressGallerySection: View {
        var body: some View {
            GallerySection(
                "StagedProgress",
                note: "The bar and the stage that is running. Compact is one line for a row; "
                    + "stages lists every stage with a symbol and a word. The percent is whole "
                    + "and never 100 before the operation succeeded."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s22) {
                    ForEach(ComponentSamples.compactProgress, id: \.name) { sample in
                        labeled(sample.name) {
                            StagedProgress(sample.value, layout: .compact)
                        }
                    }
                    ForEach(ComponentSamples.stagesProgress, id: \.name) { sample in
                        labeled(sample.name) {
                            StagedProgress(sample.value, layout: .stages)
                        }
                    }
                    if let first = ComponentSamples.stagesProgress.first {
                        labeled("\(first.name), inspector width") {
                            StagedProgress(first.value, layout: .stages)
                        }
                        .frame(maxWidth: ComponentsGalleryWidth.inspector, alignment: .leading)
                    }
                }
            }
        }

        private func labeled(_ name: String, @ViewBuilder content: () -> some View) -> some View {
            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text(name)
                    .font(.subheadline.weight(.semibold))
                content()
            }
            .frame(maxWidth: 560, alignment: .leading)
        }
    }
#endif
