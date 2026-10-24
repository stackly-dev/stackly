#if DEBUG
    import SwiftUI

    /// The gallery sections of milestone 4: the log view and the diff view.
    struct LogComponentsSections: View {
        var body: some View {
            LogViewGallerySection()
            DiffViewGallerySection()
        }
    }

    // MARK: - Log

    /// The LogView section: every level, long lines, long sources, empty columns, and a
    /// live log that follows.
    struct LogViewGallerySection: View {
        @State private var staticFollow = false
        @State private var liveFollow = true
        @State private var liveLines = (1...12).map { ComponentSamples.liveLogLine($0) }

        var body: some View {
            GallerySection(
                "LogView",
                note: "One line per row in monospace. The time and source columns line up; "
                    + "a longer source shows in full. Warnings and errors always have a "
                    + "symbol. The second log follows new lines until you scroll up."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s22) {
                    labeled("Normal, warning, error, long line, long source, no time or source") {
                        LogView(ComponentSamples.logStates, follow: $staticFollow)
                            .frame(maxWidth: 720, minHeight: 260, maxHeight: 260)
                    }
                    labeled("Live: follow is \(liveFollow ? "on" : "off")") {
                        VStack(alignment: .leading, spacing: Spacing.s8) {
                            LogView(liveLines, follow: $liveFollow)
                                .frame(maxWidth: 720, minHeight: 160, maxHeight: 160)
                            HStack(spacing: Spacing.s12) {
                                Button("Add Line") {
                                    liveLines.append(
                                        ComponentSamples.liveLogLine(liveLines.count + 1))
                                }
                                Toggle("Follow", isOn: $liveFollow)
                            }
                        }
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
        }
    }

    // MARK: - Diff

    /// The DiffView section: added, removed, and unchanged lines, long lines, and empty.
    struct DiffViewGallerySection: View {
        var body: some View {
            GallerySection(
                "DiffView",
                note: "A sign column that always shows, so a change is never only a color: "
                    + "“+” for a line that was added and “−” for one that was removed. "
                    + "Unchanged lines have secondary text."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s22) {
                    labeled("Added, removed, and unchanged") {
                        DiffView(
                            ComponentSamples.diffLines, emptyText: ComponentSamples.diffEmptyText)
                    }
                    labeled("Long lines wrap") {
                        DiffView(
                            ComponentSamples.diffLongLines,
                            emptyText: ComponentSamples.diffEmptyText)
                    }
                    labeled("Empty") {
                        DiffView([], emptyText: ComponentSamples.diffEmptyText)
                    }
                }
                .frame(maxWidth: 560, alignment: .leading)
            }
        }

        private func labeled(_ name: String, @ViewBuilder content: () -> some View) -> some View {
            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text(name)
                    .font(.subheadline.weight(.semibold))
                content()
            }
        }
    }
#endif
