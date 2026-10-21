#if DEBUG
    import StacklyClient
    import SwiftUI

    /// The gallery sections of milestone 1: tile, key value, copy, console, actions,
    /// feedback.
    struct BaseComponentsSections: View {
        var body: some View {
            TileGallerySection()
            KeyValueGallerySection()
            CopyGallerySection()
            ConsoleGallerySection()
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
                    CardSection("Connection") {
                        HStack(spacing: Spacing.s8) {
                            LabeledContent("Endpoint") {
                                Text("127.0.0.1:5432").font(.callout.monospaced())
                            }
                            CopyButton(
                                "127.0.0.1:5432", what: "endpoint", style: .icon,
                                announcement: "Address copied.")
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

    // MARK: - Copy and console

    /// The CopyButton section: both styles at rest and confirmed, and a live button.
    struct CopyGallerySection: View {
        var body: some View {
            GallerySection(
                "CopyButton",
                note: "Pressing writes the text, then shows Copied with a checkmark for two "
                    + "seconds without moving the layout. The last four are static states."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s12) {
                    HStack(spacing: Spacing.s22) {
                        CopyButton("127.0.0.1:5432", what: "address", key: "gallery")
                        CopyButton("127.0.0.1:5432", what: "address", style: .icon)
                    }
                    HStack(spacing: Spacing.s22) {
                        CopyButtonFace(style: .text, what: "address", isConfirming: false) {}
                        CopyButtonFace(style: .text, what: "address", isConfirming: true) {}
                        CopyButtonFace(style: .icon, what: "address", isConfirming: false) {}
                        CopyButtonFace(style: .icon, what: "address", isConfirming: true) {}
                    }
                }
            }
        }
    }

    /// The ConsoleBlock section: a command with Copy, a command that wraps, and text.
    struct ConsoleGallerySection: View {
        var body: some View {
            GallerySection(
                "ConsoleBlock",
                note: "Selectable monospace text on the console surface. Long lines wrap; "
                    + "there is no horizontal scroll. Copy takes the command without “$ ”."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s12) {
                    ConsoleBlock(command: ComponentSamples.command)
                    ConsoleBlock(command: ComponentSamples.longCommand)
                    ConsoleBlock(
                        text: ComponentSamples.multilineText,
                        accessibilityLabel: "Example",
                        copy: ConsoleCopy(what: "example")
                    )
                }
            }
        }
    }
#endif
