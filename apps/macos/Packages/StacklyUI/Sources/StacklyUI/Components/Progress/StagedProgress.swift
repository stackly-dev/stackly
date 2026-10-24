import SwiftUI

/// How a ``StagedProgress`` is laid out.
public enum StagedProgressLayout {
    /// One line of text with the bar under it, for a row.
    case compact
    /// The bar, then every stage with its symbol, wrapping to the next line.
    case stages
}

/// The progress of an operation with stages: a bar, the stage that is running, and the
/// outcome (spec 0067, AC-15).
/// The value comes from ``StagedProgressValue/from(operation:stages:)``.
public struct StagedProgress: View {

    private let value: StagedProgressValue
    private let layout: StagedProgressLayout

    /// Creates a staged progress.
    /// - Parameters:
    ///   - value: What to show.
    ///   - layout: The compact line or the list of stages.
    public init(
        _ value: StagedProgressValue,
        layout: StagedProgressLayout
    ) {
        self.value = value
        self.layout = layout

    }

    /// The view's content.
    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            switch layout {
            case .compact:
                compactText
                bar
            case .stages:
                bar
                StageFlowLayout {
                    ForEach(value.stages, id: \.key) { stage in
                        StageLabel(stage: stage)
                    }
                }
            }
        }
        .animation(StacklyMotion.progress(), value: value.fraction)

    }

    /// The compact line: the text, with a symbol once the operation ended.
    private var compactText: some View {
        HStack(spacing: Spacing.s6) {
            if let symbol = endSymbol {
                Image(systemName: symbol.name)
                    .stacklySymbol(size: 13, relativeTo: .callout)
                    .foregroundStyle(symbol.tone.fill)
            }
            Text(value.compactText)
                .font(.callout)
                .monospacedDigit()
                .foregroundStyle(StacklyColor.textSecondary)
        }
    }

    /// The symbol that ends the compact line, or `nil` while the operation is going on.
    private var endSymbol: (name: String, tone: Tone)? {
        switch value.outcome {
        case .succeeded: ("checkmark.circle.fill", .positive)
        case .failed, .needsRecovery: ("xmark.circle.fill", .critical)
        case .cancelled: ("minus.circle.fill", .neutral)
        case .waiting, .running, .cancelling: nil
        }
    }

    @ViewBuilder private var bar: some View {
        if value.showsBar {
            if let fraction = value.fraction {
                ProgressView(value: fraction)
                    .progressViewStyle(.linear)
            } else {
                ProgressView()
                    .progressViewStyle(.linear)
            }
        }
    }
}

/// One stage of the `.stages` layout: its symbol and title in the look of its state.
private struct StageLabel: View {
    let stage: ProgressStage

    var body: some View {
        HStack(spacing: Spacing.s4) {
            Image(systemName: symbol)
                .stacklySymbol(size: 13, relativeTo: .callout)
                .foregroundStyle(symbolStyle)
            Text(stage.title)
                .font(.callout)
                .fontWeight(isEmphasized ? .semibold : .regular)
                .foregroundStyle(textStyle)
        }
    }

    private var symbol: String {
        switch stage.state {
        case .done: "checkmark.circle.fill"
        case .running: "circle.inset.filled"
        case .pending: "circle"
        case .failed: "xmark.circle.fill"
        case .cancelled: "minus.circle.fill"
        }
    }

    private var symbolStyle: AnyShapeStyle {
        switch stage.state {
        case .done: AnyShapeStyle(Tone.positive.fill)
        case .running: AnyShapeStyle(Tone.informative.fill)
        case .pending: AnyShapeStyle(StacklyColor.textSecondary)
        case .failed: AnyShapeStyle(Tone.critical.fill)
        case .cancelled: AnyShapeStyle(Tone.neutral.fill)
        }
    }

    private var textStyle: AnyShapeStyle {
        switch stage.state {
        case .done, .running: AnyShapeStyle(.primary)
        case .pending, .cancelled: AnyShapeStyle(StacklyColor.textSecondary)
        case .failed: AnyShapeStyle(Tone.critical.text)
        }
    }

    private var isEmphasized: Bool {
        stage.state == .running || stage.state == .failed
    }
}

#if DEBUG
    #Preview("Light") {
        GalleryPageLayout { StagedProgressGallerySection() }.galleryPreview(.light)
    }

    #Preview("Dark") {
        GalleryPageLayout { StagedProgressGallerySection() }.galleryPreview(.dark)
    }

    #Preview("Narrow width") {
        GalleryPageLayout { StagedProgressGallerySection() }.galleryPreview(.narrowWidth)
    }
#endif
