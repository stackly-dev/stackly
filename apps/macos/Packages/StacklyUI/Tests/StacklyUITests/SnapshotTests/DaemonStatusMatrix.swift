import SwiftUI

@testable import StacklyUI

/// A test only view that stacks one row per presence of the daemon status table of spec
/// 0013: the presence name, the sidebar footer at 236 pt, and the window banner, or "no
/// banner" (spec 0068, AC-8). One picture shows every row, so a change to one row is
/// easy to see in a review.
struct DaemonStatusMatrix: View {
    /// One row of the matrix.
    struct Row: Identifiable {
        let name: String
        let status: ShellStatus
        var id: String { name }
    }

    /// The width of the footer, the ideal sidebar width of spec 0013.
    static let footerWidth: CGFloat = 236

    static let rows: [Row] = {
        let now = FixtureClock.now
        let lastKnown = FixtureClock.minutesAgo(5)
        return [
            Row(name: "connected(ready)", status: .connected(.ready)),
            Row(name: "connected(degraded)", status: .connected(.degraded)),
            Row(name: "connected(unknown)", status: .connected(.unknown)),
            Row(
                name: "refreshing after connecting",
                status: .refreshing(after: .connecting(slow: false, since: now))),
            Row(
                name: "refreshing after unreachable",
                status: .refreshing(after: .unreachable(lastKnownAt: lastKnown, since: now))),
            Row(name: "starting", status: .starting(since: now)),
            Row(name: "stopping", status: .stopping(since: now)),
            Row(name: "connecting(slow: false)", status: .connecting(slow: false, since: now)),
            Row(name: "connecting(slow: true)", status: .connecting(slow: true, since: now)),
            Row(
                name: "unreachable, never loaded",
                status: .unreachable(lastKnownAt: nil, since: now)),
            Row(
                name: "unreachable, last known",
                status: .unreachable(lastKnownAt: lastKnown, since: now)),
            Row(
                name: "incompatible, major mismatch",
                status: .incompatible(
                    .majorMismatch(daemonProtocol: "2.0", appProtocol: "1.8"), since: now)),
            Row(
                name: "incompatible, handshake rejected",
                status: .incompatible(
                    .handshakeRejected(code: "request.invalid_params"), since: now)
            ),
            Row(
                name: "untrusted, endpoint",
                status: .untrusted(.endpoint(reason: "group_or_other_access"), since: now)),
            Row(name: "untrusted, peer rejected", status: .untrusted(.peerRejected, since: now)),
            Row(name: "recovery", status: .recovery(since: now)),
        ]
            + ShellStatus.Misconfiguration.allCases.map {
                Row(name: "misconfigured, \($0)", status: .misconfigured($0, since: now))
            }
    }()

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Self.rows) { row in
                MatrixRow(row: row)
                Divider()
            }
        }
    }
}

private struct MatrixRow: View {
    let row: DaemonStatusMatrix.Row

    var body: some View {
        let presentation = ShellPresentation(status: row.status)
        VStack(alignment: .leading, spacing: 0) {
            Text(row.name)
                .font(.caption.monospaced())
                .padding(.horizontal, Spacing.s16)
                .padding(.top, Spacing.s8)
            HStack(alignment: .top, spacing: 0) {
                DaemonFooter(status: presentation.footer)
                    .frame(width: DaemonStatusMatrix.footerWidth, alignment: .leading)
                if let banner = presentation.banner {
                    DaemonBanner(
                        banner, isReconnecting: false, reconnect: {}, openDiagnostics: {})
                } else {
                    Text("no banner")
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .padding(Spacing.s16)
                }
            }
        }
    }
}
