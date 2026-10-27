#if DEBUG
    import AppKit
    import SwiftUI

    /// The Tokens page of the design gallery: every color token, tone, text style,
    /// spacing value, radius, scaled size, and the crossfade.
    struct TokensGalleryPage: View {
        var body: some View {
            GalleryPageLayout {
                ColorSection()
                ToneSection()
                TypographySection()
                SpacingSection()
                ScaledSizeSection()
                MotionSection()
            }
        }
    }

    // MARK: - Color

    private struct ColorSection: View {
        private struct Swatch: Identifiable {
            let name: String
            let source: String
            let token: ColorToken

            var id: String { name }
        }

        private let swatches = [
            Swatch(name: "surfaceWindow", source: "--win", token: StacklyColor.surfaceWindow),
            Swatch(name: "surfaceCard", source: "--card", token: StacklyColor.surfaceCard),
            Swatch(name: "surfaceConsole", source: "--console", token: StacklyColor.surfaceConsole),
            Swatch(name: "textSecondary", source: "--text2", token: StacklyColor.textSecondary),
        ]

        private let columns = [
            GridItem(.adaptive(minimum: 260), spacing: Spacing.s12, alignment: .top)
        ]

        var body: some View {
            GallerySection(
                "Color",
                note: "Custom tokens hold light and dark values. Every other "
                    + "neutral is a system color."
            ) {
                LazyVGrid(columns: columns, alignment: .leading, spacing: Spacing.s12) {
                    ForEach(swatches) { swatch in
                        TokenSwatch(name: swatch.name, source: swatch.source, token: swatch.token)
                    }
                    DecorativeSwatch()
                }
                TextOnSurfaces()
            }
        }
    }

    private struct TokenSwatch: View {
        let name: String
        let source: String
        let token: ColorToken

        var body: some View {
            HStack(alignment: .top, spacing: Spacing.s12) {
                RoundedRectangle(cornerRadius: Radius.small.value)
                    .fill(token)
                    .overlay {
                        RoundedRectangle(cornerRadius: Radius.small.value)
                            .strokeBorder(Color(nsColor: .separatorColor))
                    }
                    .frame(width: 44, height: 44)

                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(name)
                        .font(.body.weight(.semibold))
                    Text("StacklyColor.\(name) · mockup \(source)")
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    Text(hexValues)
                        .stacklyCode()
                        .foregroundStyle(StacklyColor.textSecondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.cardInsets)
            .cardSurface()
        }

        private var hexValues: String {
            let values = [
                token.components(scheme: .light),
                token.components(scheme: .dark),
            ]
            return values.map(\.galleryHex).joined(separator: " ")
        }
    }

    private struct DecorativeSwatch: View {
        var body: some View {
            HStack(alignment: .top, spacing: Spacing.s12) {
                Image(systemName: "chevron.right")
                    .stacklySymbol(size: 22, relativeTo: .body)
                    .foregroundStyle(StacklyColor.decorative)
                    .frame(width: 44, height: 44)

                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text("decorative")
                        .font(.body.weight(.semibold))
                    Text("StacklyColor.decorative · system tertiary label")
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    Text("Disabled text and decorative icons only")
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.cardInsets)
            .cardSurface()
        }
    }

    private struct TextOnSurfaces: View {
        private struct Surface: Identifiable {
            let name: String
            let style: AnyShapeStyle

            var id: String { name }
        }

        private let surfaces = [
            Surface(name: "surfaceWindow", style: AnyShapeStyle(StacklyColor.surfaceWindow)),
            Surface(name: "surfaceCard", style: AnyShapeStyle(StacklyColor.surfaceCard)),
            Surface(name: "surfaceConsole", style: AnyShapeStyle(StacklyColor.surfaceConsole)),
            Surface(
                name: "windowBackgroundColor",
                style: AnyShapeStyle(Color(nsColor: .windowBackgroundColor))
            ),
            Surface(
                name: "controlBackgroundColor",
                style: AnyShapeStyle(Color(nsColor: .controlBackgroundColor))
            ),
        ]

        private let columns = [
            GridItem(.adaptive(minimum: 180), spacing: Spacing.s8, alignment: .top)
        ]

        var body: some View {
            LazyVGrid(columns: columns, alignment: .leading, spacing: Spacing.s8) {
                ForEach(surfaces) { surface in
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Text("Primary text")
                            .font(.body)
                        Text("Secondary text")
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                        Text(surface.name)
                            .font(.footnote)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(Spacing.s12)
                    .background(
                        surface.style,
                        in: RoundedRectangle(cornerRadius: Radius.small.value)
                    )
                    .overlay {
                        RoundedRectangle(cornerRadius: Radius.small.value)
                            .strokeBorder(Color(nsColor: .separatorColor))
                    }
                }
            }
        }
    }

    // MARK: - Tone

    private struct ToneSample {
        let tone: Tone
        let meaning: String
        let badge: String
        let bannerTitle: String
        let bannerMessage: String

        static let all = [
            ToneSample(
                tone: .positive,
                meaning: "Running and ready, verified, tested",
                badge: "Running",
                bannerTitle: "Backup verified",
                bannerMessage: "pg17-dev · restored to a scratch instance and checked."
            ),
            ToneSample(
                tone: .caution,
                meaning: "Starting, recovering, unknown, drift, partial, stale",
                badge: "Starting…",
                bannerTitle: "Showing the last known state from 14:02.",
                bannerMessage: "Couldn’t refresh: the request timed out."
            ),
            ToneSample(
                tone: .critical,
                meaning: "Failed, error, conflict",
                badge: "Failed",
                bannerTitle: "mysql-erp failed to start",
                bannerMessage: "Port 3306 is already in use by another process."
            ),
            ToneSample(
                tone: .neutral,
                meaning: "Stopped, retained, done, cancelled, not tested",
                badge: "Stopped",
                bannerTitle: "valkey-erp is stopped",
                bannerMessage: "Data is kept."
            ),
            ToneSample(
                tone: .informative,
                meaning: "An operation or install in progress",
                badge: "Running",
                bannerTitle: "Installing PostgreSQL 17",
                bannerMessage: "Downloading and verifying the runtime."
            ),
        ]
    }

    private struct ToneSection: View {
        var body: some View {
            GallerySection(
                "Tones",
                note: "Fill is a system color for dots, icons, and tints. Text colors pass AA "
                    + "on plain surfaces and on their own tint."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s12) {
                    ForEach(ToneSample.all, id: \.tone) { sample in
                        ToneRow(sample: sample)
                    }
                }
            }
        }
    }

    private struct ToneRow: View {
        private let dotSize = ScaledSize.statusDot.base
        private let badgeHeight = ScaledSize.badgeHeight.base

        let sample: ToneSample

        var body: some View {
            VStack(alignment: .leading, spacing: Spacing.s12) {
                HStack(spacing: Spacing.s8) {
                    Circle()
                        .fill(sample.tone.fill)
                        .frame(width: dotSize, height: dotSize)

                    Image(systemName: sample.tone.symbol)
                        .foregroundStyle(sample.tone.fill)

                    Text(verbatim: "\(sample.tone)")
                        .font(.body.weight(.semibold))
                        .foregroundStyle(sample.tone.text)
                    Text(sample.meaning)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: Spacing.s12) {
                        badge
                        banner(on: .card)
                        banner(on: .window)
                    }
                    VStack(alignment: .leading, spacing: Spacing.s8) {
                        badge
                        banner(on: .card)
                        banner(on: .window)
                    }
                }
            }
            .padding(Spacing.cardInsets)
            .cardSurface()
        }

        private var badge: some View {
            Text(sample.badge)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(sample.tone.text)
                .padding(.horizontal, Spacing.s8)
                .frame(minHeight: badgeHeight)
                .toneSurface(sample.tone, strength: .badge)
        }

        private func banner(on base: SurfaceToken) -> some View {
            HStack(alignment: .top, spacing: Spacing.s8) {
                Image(systemName: sample.tone.symbol)
                    .stacklySymbol(.bannerSymbol)
                    .foregroundStyle(sample.tone.fill)

                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(sample.bannerTitle)
                        .font(.body.weight(.semibold))
                    Text(sample.bannerMessage)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    Text(verbatim: "tone text on .\(base)")
                        .font(.callout)
                        .foregroundStyle(sample.tone.text)
                }
                .fixedSize(horizontal: false, vertical: true)
            }
            .frame(minWidth: 220, maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.s12)
            .toneSurface(sample.tone, on: base, strength: .banner)

        }
    }

    // MARK: - Typography

    private struct TypographySection: View {
        private struct Role: Identifiable {
            let role: String
            let mockup: String
            let api: String
            let font: Font
            let sample: String
            var isSecondary = false

            var id: String { api }
        }

        @Environment(\.fontResolutionContext) private var fontContext

        private let roles = [
            Role(
                role: "Onboarding title",
                mockup: "28, 700",
                api: ".largeTitle.bold()",
                font: .largeTitle.bold(),
                sample: "Welcome to Stackly"
            ),
            Role(
                role: "Overview summary number",
                mockup: "28, 600",
                api: ".largeTitle.weight(.semibold).monospacedDigit()",
                font: .largeTitle.weight(.semibold).monospacedDigit(),
                sample: "12"
            ),
            Role(
                role: "Onboarding step title",
                mockup: "24, 700",
                api: ".title.bold()",
                font: .title.bold(),
                sample: "Background Agent"
            ),
            Role(
                role: "Metric value",
                mockup: "20, 600",
                api: ".title.weight(.semibold).monospacedDigit()",
                font: .title.weight(.semibold).monospacedDigit(),
                sample: "412 MB"
            ),
            Role(
                role: "Page placeholder title, detail panel title",
                mockup: "17, 600 to 700",
                api: ".title2.weight(.semibold)",
                font: .title2.weight(.semibold),
                sample: "pg17-dev"
            ),
            Role(
                role: "Section placeholder title, large card title",
                mockup: "15, 600 to 700",
                api: ".title3.weight(.semibold)",
                font: .title3.weight(.semibold),
                sample: "Backups"
            ),
            Role(
                role: "Section heading",
                mockup: "13, 700",
                api: ".headline",
                font: .headline,
                sample: "Needs Attention"
            ),
            Role(
                role: "Body text",
                mockup: "13, 400",
                api: ".body",
                font: .body,
                sample: "PostgreSQL 17 on port 5432"
            ),
            Role(
                role: "Emphasized row title",
                mockup: "13, 600",
                api: ".body.weight(.semibold)",
                font: .body.weight(.semibold),
                sample: "mysql-erp"
            ),
            Role(
                role: "Secondary text, banner body",
                mockup: "12",
                api: ".callout",
                font: .callout,
                sample: "Showing the last known state from 14:02.",
                isSecondary: true
            ),
            Role(
                role: "Monospace table cell (.stacklyCode())",
                mockup: "12",
                api: ".callout.monospaced()",
                font: .callout.monospaced(),
                sample: "127.0.0.1:5432"
            ),
            Role(
                role: "Toolbar subtitle, caption",
                mockup: "11",
                api: ".subheadline",
                font: .subheadline,
                sample: "3 running · 1 failed",
                isSecondary: true
            ),
            Role(
                role: "Tag, sidebar group header",
                mockup: "11",
                api: ".subheadline.weight(.semibold)",
                font: .subheadline.weight(.semibold),
                sample: "Library"
            ),
            Role(
                role: "Smallest metadata",
                mockup: "10 to 10.5",
                api: ".footnote",
                font: .footnote,
                sample: "Updated 14:02",
                isSecondary: true
            ),
        ]

        var body: some View {
            GallerySection(
                "Typography",
                note: "Semantic text styles only. Sizes shown are what macOS resolves now."
            ) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(roles) { role in
                        VStack(alignment: .leading, spacing: Spacing.s4) {
                            Text(role.sample)
                                .font(role.font)
                                .foregroundStyle(
                                    role.isSecondary
                                        ? AnyShapeStyle(StacklyColor.textSecondary)
                                        : AnyShapeStyle(.primary)
                                )
                            Text(caption(for: role))
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(Spacing.rowInsets)
                        if role.id != roles.last?.id {
                            Divider()
                        }
                    }
                }
                .cardSurface()
            }
        }

        private func caption(for role: Role) -> String {
            let size = role.font.resolve(in: fontContext).pointSize
            let points = Double(size).formatted(.number.precision(.fractionLength(0...1)))
            return "\(role.role) · \(role.api) · \(points) pt · mockup \(role.mockup)"
        }
    }

    // MARK: - Spacing and radius

    private struct SpacingSection: View {
        private let scale: [(String, CGFloat)] = [
            ("s2", Spacing.s2), ("s4", Spacing.s4), ("s6", Spacing.s6), ("s8", Spacing.s8),
            ("s12", Spacing.s12), ("s14", Spacing.s14), ("s16", Spacing.s16),
            ("s22", Spacing.s22),
        ]

        var body: some View {
            GallerySection(
                "Spacing and radius",
                note: "Fixed points that never scale. Rows have a minimum height, never a "
                    + "fixed one."
            ) {
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: Spacing.s12) {
                        spacingCard
                        radiusCard
                    }
                    VStack(alignment: .leading, spacing: Spacing.s12) {
                        spacingCard
                        radiusCard
                    }
                }
            }
        }

        private var spacingCard: some View {
            VStack(alignment: .leading, spacing: Spacing.s6) {
                ForEach(scale, id: \.0) { name, value in
                    HStack(spacing: Spacing.s8) {
                        Text("Spacing.\(name)")
                            .stacklyCode()
                            .frame(minWidth: 110, alignment: .leading)
                        Rectangle()
                            .fill(Tone.informative.fill)
                            .frame(width: value, height: 10)

                        Text("\(Int(value)) pt")
                            .font(.callout)
                            .stacklyNumeric()
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
                Text(
                    "Page inset 22 · section gap 22 · card 14 × 16 · row 11 × 14 · "
                        + "compact row 9 × 14"
                )
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            }
            .frame(minWidth: 260, maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.cardInsets)
            .cardSurface()
        }

        private var radiusCard: some View {
            HStack(alignment: .top, spacing: Spacing.s12) {
                ForEach(Radius.allCases, id: \.self) { radius in
                    VStack(spacing: Spacing.s6) {
                        Color.clear
                            .frame(width: 72, height: 48)
                            .cardSurface(radius: radius)

                        Text(verbatim: "Radius.\(radius) · \(Int(radius.value)) pt")
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
            }
            .frame(minWidth: 260, maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.cardInsets)
            .background(StacklyColor.surfaceWindow)
        }
    }

    // MARK: - Scaled sizes

    private struct ScaledSizeSection: View {
        private let sizes: [(String, ScaledSize)] = [
            ("statusDot", .statusDot),
            ("bannerSymbol", .bannerSymbol),
            ("badgeHeight", .badgeHeight),
            ("placeholderPageSymbol", .placeholderPageSymbol),
            ("placeholderSectionSymbol", .placeholderSectionSymbol),
            ("rowMinHeight", .rowMinHeight),
        ]

        var body: some View {
            GallerySection(
                "Scaled sizes",
                note: "Named layout sizes. Custom hit targets are at least "
                    + "\(Int(HitTarget.minimum)) × \(Int(HitTarget.minimum)) pt."
            ) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(sizes, id: \.0) { name, size in
                        ScaledSizeRow(name: name, size: size)
                        if name != sizes.last?.0 {
                            Divider()
                        }
                    }
                }
                .cardSurface()
            }
        }
    }

    private struct ScaledSizeRow: View {
        private let scaled: CGFloat

        let name: String
        let size: ScaledSize

        init(name: String, size: ScaledSize) {
            self.name = name
            self.size = size
            scaled = size.base
        }

        var body: some View {
            HStack(spacing: Spacing.s12) {
                Circle()
                    .fill(Tone.informative.fill)
                    .frame(width: scaled, height: scaled)
                    .frame(width: 48)

                Text("ScaledSize.\(name)")
                    .stacklyCode()
                Text(
                    verbatim:
                        "\(Int(size.base)) pt, relative to .\(size.textStyle), now \(Int(scaled)) pt"
                )
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.compactRowInsets)
        }
    }

    // MARK: - Motion

    private struct MotionSection: View {
        @State private var isBannerShown = true

        var body: some View {
            GallerySection(
                "Motion",
                note: "Status changes, banners, and state changes crossfade in 0.2 s, also "
                    + "in the app. Nothing moves, slides, or scales."
            ) {
                VStack(alignment: .leading, spacing: Spacing.s12) {
                    Button(isBannerShown ? "Hide Banner" : "Show Banner") {
                        isBannerShown.toggle()
                    }
                    GalleryMotionBanner(isShown: isBannerShown)
                }
            }
        }
    }

    /// The motion sample's banner, including the space it occupies in the page.
    struct GalleryMotionBanner: View {
        let isShown: Bool

        var body: some View {
            // Keep the sample's height so hiding it cannot move a page scrolled to its bottom.
            Text("The last known state is from 14:02.")
                .font(.body.weight(.semibold))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(Spacing.s12)
                .toneSurface(.caution, on: .window, strength: .banner)
                .opacity(isShown ? 1 : 0)
                .animation(StacklyMotion.crossfadeAnimation, value: isShown)
        }
    }

    extension SRGBColor {
        /// The color as `#RRGGBB`.
        fileprivate var galleryHex: String {
            func byte(_ value: Double) -> Int {
                Int((min(max(value, 0), 1) * 255).rounded())
            }
            return String(format: "#%02X%02X%02X", byte(red), byte(green), byte(blue))
        }
    }

    #Preview("Light") {
        TokensGalleryPage().galleryPreview(.light)
    }

    #Preview("Dark") {
        TokensGalleryPage().galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        TokensGalleryPage().galleryPreview(.narrowWidth)
    }
#endif
