import Charts
import StacklyClient
import SwiftUI

/// The CPU graph of the Metrics tab (spec 0028, AC-13): one bar per sample of the selected
/// scope, newest on the right. A sample whose CPU could not be measured leaves a gap; it is
/// never drawn as zero.
struct MetricsGraph: View {
    /// One bar.
    struct Point: Equatable, Identifiable {
        /// The sample's `seq`.
        let seq: UInt64
        /// CPU as a percent of one core.
        let cpu: Double

        var id: UInt64 { seq }
    }

    let samples: [MetricsSample]
    let scope: InstanceMetricsModel.Scope
    let capacity: UInt64
    let intervalSeconds: UInt32

    /// The bars of `samples` for `scope`: only samples with a measured CPU.
    static func points(_ samples: [MetricsSample], scope: InstanceMetricsModel.Scope) -> [Point] {
        samples.compactMap { sample in
            let usage = scope == .main ? sample.main : sample.tree
            guard case .value(let cpu) = usage.cpu else { return nil }
            return Point(seq: sample.seq, cpu: cpu)
        }
    }

    /// The top of the vertical axis: 100 percent, or the next multiple of 100 above the
    /// highest bar, since a busy tree is over 100.
    static func axisTop(_ points: [Point]) -> Double {
        let peak = points.map(\.cpu).max() ?? 0
        return max(100, (peak / 100).rounded(.up) * 100)
    }

    var body: some View {
        let points = Self.points(samples, scope: scope)
        VStack(alignment: .leading, spacing: Spacing.s12) {
            HStack(alignment: .firstTextBaseline) {
                Text(MetricsFormat.graphTitle(capacity: capacity))
                    .font(.callout.weight(.semibold))
                Spacer(minLength: Spacing.s12)
                Text(MetricsFormat.sampledEvery(intervalSeconds))
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            if points.isEmpty {
                Text(samples.isEmpty ? "Waiting for the first sample" : "No CPU measured yet")
                    .foregroundStyle(StacklyColor.textSecondary)
                    .frame(maxWidth: .infinity, minHeight: 140)
            } else {
                chart(points)
            }
        }
        .padding(Spacing.cardInsets)
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardSurface()
    }

    private func chart(_ points: [Point]) -> some View {
        let newest = Double(samples.last?.seq ?? 0)
        let oldest = max(1, newest - Double(capacity) + 1)
        return Chart(points) { point in
            RectangleMark(
                xStart: .value("From", Double(point.seq) - 0.4),
                xEnd: .value("To", Double(point.seq) + 0.4),
                yStart: .value("Zero", 0),
                yEnd: .value("CPU", point.cpu))
        }
        .foregroundStyle(Tone.informative.fill)
        .chartXScale(domain: (oldest - 0.5)...(newest + 0.5))
        .chartYScale(domain: 0...Self.axisTop(points))
        .chartXAxis(.hidden)
        .chartYAxis {
            AxisMarks(position: .leading) { value in
                AxisGridLine()
                AxisValueLabel {
                    if let percent = value.as(Double.self) {
                        Text("\(Int(percent))%")
                    }
                }
            }
        }
        .frame(height: 140)
    }
}
