#if DEBUG
    import SwiftUI

    /// The Status page of the design gallery: `StatusLabel` for every row of the instance
    /// status table in both contexts, `ToneBadge` for every operation state, and the
    /// section icons.
    struct StatusGalleryPage: View {
        var body: some View {
            GalleryPageLayout {
                InstanceStatusSection()
                RowContextSection()
                OperationStatusSection()
                SectionIconSection()
            }
        }
    }

    // MARK: - Samples

    /// One input of the instance status table, with the appearance it maps to.
    private struct InstanceSample: Identifiable {
        let input: String
        let appearance: StatusAppearance

        var id: String { input }

        init(
            _ input: String,
            _ lifecycle: LifecycleStatus,
            _ readiness: ReadinessStatus = .unknown,
            _ health: HealthStatus = .unknown
        ) {
            self.input = input
            appearance = .instance(lifecycle: lifecycle, readiness: readiness, health: health)
        }

        /// Every row of the table, plus the longest daemon detail for the narrow width.
        static let all = [
            InstanceSample("uninitialized", .uninitialized),
            InstanceSample("initializing", .initializing),
            InstanceSample("stopped", .stopped),
            InstanceSample("starting", .starting),
            InstanceSample("running, awaiting probe", .running, .awaitingProbe),
            InstanceSample("running, not ready", .running, .notReady(detail: "Port in use")),
            InstanceSample("running, not ready, no detail", .running, .notReady(detail: nil)),
            InstanceSample(
                "running, not ready, long detail",
                .running,
                .notReady(
                    detail: "Probe failed: connection refused on 127.0.0.1:5433 after 3 "
                        + "attempts while the server replays its write ahead log"
                )
            ),
            InstanceSample("running, readiness unknown", .running, .unknown),
            InstanceSample("running, ready, healthy", .running, .ready, .healthy),
            InstanceSample("running, ready, degraded", .running, .ready, .degraded),
            InstanceSample("running, ready, unhealthy", .running, .ready, .unhealthy),
            InstanceSample("recovering", .recovering),
            InstanceSample("stopping", .stopping),
            InstanceSample("failed", .failed),
            InstanceSample("unknown", .unknown),
            InstanceSample("retained", .retained),
        ]
    }

    /// One row of the operation status table, with the appearance it maps to.
    private struct OperationSample: Identifiable {
        let input: String
        let appearance: StatusAppearance

        var id: String { input }

        init(_ input: String, _ state: OperationStatus, partial: Bool = false) {
            self.input = input
            appearance = .operation(state: state, partial: partial)
        }

        static let all = [
            OperationSample("queued", .queued),
            OperationSample("running", .running),
            OperationSample("cancelRequested", .cancelRequested),
            OperationSample("succeeded", .succeeded),
            OperationSample("succeeded, partial", .succeeded, partial: true),
            OperationSample("failed", .failed),
            OperationSample("cancelled", .cancelled),
            OperationSample("needsRecovery", .needsRecovery),
        ]
    }

    /// The input column of a sample table.
    private struct InputText: View {
        let text: String

        init(_ text: String) {
            self.text = text
        }

        var body: some View {
            Text(text)
                .font(.callout.monospaced())
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize()
        }
    }

    /// A column heading of a sample table.
    private struct ColumnHeading: View {
        let text: String

        init(_ text: String) {
            self.text = text
        }

        var body: some View {
            Text(text)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(StacklyColor.textSecondary)

        }
    }

    // MARK: - Instance status

    private struct InstanceStatusSection: View {
        var body: some View {
            GallerySection(
                "Instance Status",
                note: "StatusLabel in the standard context (cards, banners, sheets, panels). "
                    + "The label is never cut; the detail truncates first."
            ) {
                Grid(
                    alignment: .leading, horizontalSpacing: Spacing.s16, verticalSpacing: Spacing.s8
                ) {
                    GridRow {
                        ColumnHeading("Lifecycle, readiness, health")
                        ColumnHeading("Standard")
                    }
                    Divider()
                    ForEach(InstanceSample.all) { sample in
                        GridRow {
                            InputText(sample.input)
                            StatusLabel(sample.appearance)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
                .padding(Spacing.cardInsets)
                .cardSurface()
            }
        }
    }

    // MARK: - Row context

    private struct RowContextSection: View {
        @State private var selection: InstanceSample.ID? = InstanceSample.all[9].id

        var body: some View {
            GallerySection(
                "Instance Status in Rows",
                note: "StatusLabel(context: .row) in a native List: the label and detail use "
                    + "the primary color. Select a row and focus the list to see the dot and "
                    + "symbols turn primary too."
            ) {
                List(InstanceSample.all, selection: $selection) { sample in
                    HStack(spacing: Spacing.s12) {
                        Text(sample.input)
                            .lineLimit(1)
                            .frame(width: 210, alignment: .leading)
                        StatusLabel(sample.appearance, context: .row)
                            .font(.callout)
                        Spacer(minLength: 0)
                    }
                }
                .listStyle(.inset)
                .frame(height: 440)
                .clipShape(RoundedRectangle(cornerRadius: Radius.medium.value))
            }
        }
    }

    // MARK: - Operation status

    private struct OperationStatusSection: View {
        var body: some View {
            GallerySection(
                "Operation Status",
                note: "ToneBadge for every operation state."
            ) {
                Grid(
                    alignment: .leading, horizontalSpacing: Spacing.s16, verticalSpacing: Spacing.s8
                ) {
                    GridRow {
                        ColumnHeading("Operation state")
                        ColumnHeading("Badge")
                    }
                    Divider()
                    ForEach(OperationSample.all) { sample in
                        GridRow {
                            InputText(sample.input)
                            ToneBadge(sample.appearance)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(Spacing.cardInsets)
                .cardSurface()
            }
        }
    }

    // MARK: - Section icons

    private struct SectionIconSection: View {
        private let columns = [
            GridItem(.adaptive(minimum: 160), spacing: Spacing.s12, alignment: .leading)
        ]

        var body: some View {
            GallerySection(
                "Section Icons",
                note: "SectionIcon draws each section's symbol in its system color. It is "
                    + "the section label carries the meaning."
            ) {
                LazyVGrid(columns: columns, alignment: .leading, spacing: Spacing.s12) {
                    ForEach(SectionStyle.allCases, id: \.self) { section in
                        Label {
                            Text(verbatim: "\(section)".capitalized)
                        } icon: {
                            SectionIcon(section)
                        }
                    }
                }
                .padding(Spacing.cardInsets)
                .cardSurface()
            }
        }
    }

    #Preview("Light") {
        StatusGalleryPage().galleryPreview(.light)
    }

    #Preview("Dark") {
        StatusGalleryPage().galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        StatusGalleryPage().galleryPreview(.narrowWidth)
    }
#endif
