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
            ActionButtonGallerySection()
            ActionMenuGallerySection()
            FeedbackGallerySection()
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
                    CardSection(
                        "Backups",
                        accessory: { EmptyView() },
                        footer: { DisabledReasonText(ComponentSamples.backupReason) }
                    ) {
                        LabeledContent("Last backup", value: "Never")
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

    // MARK: - Actions

    /// The ActionButton section: primary, secondary, destructive with its confirmation,
    /// busy, disabled with a visible reason, and blocked by the mutation gate.
    struct ActionButtonGallerySection: View {
        var body: some View {
            GallerySection(
                "ActionButton",
                note: "Primary is prominent, secondary is bordered, and a destructive action is "
                    + "red and bordered, never the default, and asks first. An action the provider "
                    + "does not support stays visible, disabled, with its reason under it. While "
                    + "the daemon is unreachable a mutating button is disabled with the reason "
                    + "as help only, because the window banner writes it."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s14) {
                    HStack(alignment: .top, spacing: Spacing.s14) {
                        ActionButton(ComponentSamples.startAction, prominence: .primary)
                        ActionButton(ComponentSamples.stopAction)
                        ActionButton(ComponentSamples.removeAction)
                        ActionButton(ComponentSamples.busyAction)
                    }
                    ActionButton(ComponentSamples.unsupportedAction)
                    HStack(alignment: .top, spacing: Spacing.s14) {
                        ActionButton(ComponentSamples.startAction, prominence: .primary)
                        ActionButton(ComponentSamples.copyAction)
                    }
                    .environment(
                        \.mutationAvailability,
                        .blocked(reason: ComponentSamples.gateReason, since: .now)
                    )
                }
            }
            .actionHost()
        }
    }

    /// The ActionMenu section: the More button and the same items as a context menu.
    struct ActionMenuGallerySection: View {
        var body: some View {
            GallerySection(
                "ActionMenu",
                note: "The More button of a toolbar. A disabled item says why on a row of its "
                    + "own right under it (macOS 27: the subtitle is drawn but not exposed to "
                    + "VoiceOver, see design.md, Menus). The items are also the context menu of "
                    + "the box. A menu is checked by hand and by the XCUITest of feature 68."
            ) {
                HStack(alignment: .center, spacing: Spacing.s22) {
                    ActionMenu(ComponentSamples.menuSections)
                    Text("Right click here")
                        .padding(Spacing.s12)
                        .cardSurface()
                        .contextMenu {
                            ActionMenuItems(ComponentSamples.menuSections)
                        }
                }
            }
            .actionHost()
        }
    }

    // MARK: - Feedback

    /// The FeedbackCard section: a card per tone, with detail and code, with an action, and
    /// with a long message, then a live demo of the presenter.
    struct FeedbackGallerySection: View {
        var body: some View {
            GallerySection(
                "FeedbackCard",
                note: "An opaque card under the page. Caution, critical, and any card with an "
                    + "action stay until dismissed; the others go after a few seconds, but not "
                    + "while the pointer is over the card, keyboard focus is in it, or the app "
                    + "is in the background."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s8) {
                    ForEach(ComponentSamples.feedbacks) { feedback in
                        FeedbackCard(feedback: feedback, center: FeedbackCenter())
                    }
                    FeedbackLiveDemo()
                }
            }
            .actionHost()
        }
    }

    /// Buttons that send feedback to a center of their own, and a small area with the
    /// presenter, to try the timing, hover, and focus rules by hand.
    private struct FeedbackLiveDemo: View {
        @State private var center = FeedbackCenter()

        var body: some View {
            VStack(alignment: .leading, spacing: Spacing.s8) {
                HStack(spacing: Spacing.s8) {
                    Button("Success") { center.show(.success("Instance started.")) }
                    Button("Started") { center.show(.started("Installing PostgreSQL 18.1.")) }
                    Button("Not Sent") {
                        center.show(
                            Feedback.outcome(
                                .notSent(ClientErrorCatalog.notConnected()), success: nil,
                                showOperations: nil) ?? .neutral(""))
                    }
                    Button("Unknown Outcome") {
                        center.show(
                            Feedback.outcome(
                                .outcomeUnknown(ClientErrorCatalog.notConnected(), key: "k"),
                                success: nil, showOperations: {}) ?? .neutral(""))
                    }
                }
                Color.clear
                    .frame(height: 150)
                    .environment(\.feedbackCenter, center)
                    .feedbackPresenter()
                    .background(StacklyColor.surfaceWindow)
                    .clipShape(RoundedRectangle(cornerRadius: Radius.medium.value))
                    .overlay {
                        RoundedRectangle(cornerRadius: Radius.medium.value)
                            .strokeBorder(Color(nsColor: .separatorColor))
                    }
            }
            .environment(\.feedbackCenter, center)
        }
    }
#endif
