#if DEBUG
    import StacklyClient
    import SwiftUI

    /// The gallery sections of milestone 1: tile, key value, copy, console, actions,
    /// feedback.
    struct BaseComponentsSections: View {
        var body: some View {
            TileGallerySection()
            KeyValueGallerySection()
        }
    }

    // MARK: - Tile

    /// The ProviderTile section: the six mockup tiles at four sizes, the default mark, no
    /// color, and an unrecognized color.
    struct TileGallerySection: View {
        var body: some View {
            GallerySection(
                "ProviderTile",
                note: "A system color with a bold white mark. The tile is decoration and the "
                    + "name always stands next to it. A missing mark uses the first two "
                    + "letters of the name; a missing or unrecognized color is gray."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s12) {
                    ForEach(ComponentSamples.tileSizes, id: \.self) { size in
                        HStack(spacing: Spacing.s12) {
                            ForEach(ComponentSamples.tiles, id: \.name) { tile in
                                ProviderTile(tile.spec, size: size)
                            }
                            Text("\(Int(size)) pt")
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                        }
                    }
                    HStack(spacing: Spacing.s22) {
                        labeled(ComponentSamples.tileWithoutMark)
                        labeled(ComponentSamples.tileWithoutColor)
                        labeled(ComponentSamples.tileUnrecognized)
                    }
                }
            }
        }

        private func labeled(_ sample: (name: String, spec: TileSpec)) -> some View {
            HStack(spacing: Spacing.s8) {
                ProviderTile(sample.spec, size: ProviderTile.mediumSize)
                Text(sample.name)
            }
        }
    }

    // MARK: - Key value

    /// The CardSection section: plain rows, a monospace value, a value that drops below its
    /// label, and a text footer.
    struct KeyValueGallerySection: View {
        var body: some View {
            GallerySection(
                "CardSection and key value",
                note: "Each subview is one row. The key value style puts the value flush right "
                    + "when both fit, and under the label otherwise; use the narrow width to "
                    + "see the long values drop."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s22) {
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
                    CardSection("Storage", footer: ComponentSamples.keyValueFooter) {
                        LabeledContent(ComponentSamples.longValueRow.label) {
                            Text(ComponentSamples.longValueRow.value)
                                .font(.callout.monospaced())
                        }
                        LabeledContent(ComponentSamples.digestRow.label) {
                            Text(ComponentSamples.digestRow.value)
                                .font(.callout.monospaced())
                        }
                    }
                }
                .labeledContentStyle(.stackly)
            }
        }
    }
#endif
