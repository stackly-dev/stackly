import SwiftUI

/// Lays its subviews out left to right and wraps to the next line when the next one does
/// not fit: the stages of a staged progress, 16 pt apart across and 6 pt apart down.
struct StageFlowLayout: Layout {
    /// The space between two stages on a line.
    var horizontalSpacing: CGFloat = Spacing.s16
    /// The space between two lines.
    var verticalSpacing: CGFloat = Spacing.s6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let lines = arrange(subviews, width: proposal.width ?? .infinity)
        return CGSize(
            width: lines.map(\.width).max() ?? 0,
            height: lines.reduce(0) { $0 + $1.height } + verticalSpacing
                * CGFloat(max(0, lines.count - 1)))
    }

    func placeSubviews(
        in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()
    ) {
        var y = bounds.minY
        for line in arrange(subviews, width: bounds.width) {
            var x = bounds.minX
            for item in line.items {
                subviews[item.index].place(
                    at: CGPoint(x: x, y: y), anchor: .topLeading,
                    proposal: ProposedViewSize(item.size))
                x += item.size.width + horizontalSpacing
            }
            y += line.height + verticalSpacing
        }
    }

    /// One line of stages.
    private struct Line {
        var items: [(index: Int, size: CGSize)] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func arrange(_ subviews: Subviews, width: CGFloat) -> [Line] {
        var lines: [Line] = []
        var line = Line()
        for index in subviews.indices {
            let size = subviews[index].sizeThatFits(.unspecified)
            let added = line.items.isEmpty ? size.width : horizontalSpacing + size.width
            if !line.items.isEmpty, line.width + added > width {
                lines.append(line)
                line = Line()
            }
            line.width += line.items.isEmpty ? size.width : horizontalSpacing + size.width
            line.height = max(line.height, size.height)
            line.items.append((index, size))
        }
        if !line.items.isEmpty { lines.append(line) }
        return lines
    }
}
