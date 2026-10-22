import SwiftUI

@testable import StacklyUI

/// A test only view that stacks one banner per input of the agent banner table of spec
/// 0015: the row name and the window banner `AgentBanner.content` builds at `FixtureClock.now`
/// (spec 0068, *Skenario agent*). Without a model in the environment, the agent buttons
/// are active and do nothing, like Reconnect and Open Diagnostics with empty closures.
struct AgentBannerMatrix: View {
    /// One row of the matrix.
    struct Row: Identifiable {
        let name: String
        let input: AgentBanner.Input
        var id: String { name }
    }

    static let rows: [Row] = {
        typealias Banner = AgentBannerTests
        let unreachable = ShellStatus.unreachable(
            lastKnownAt: FixtureClock.minutesAgo(5), since: FixtureClock.now)
        var refreshing = Banner.input(.notRegistered)
        refreshing.status = .refreshing(after: unreachable)
        return [
            Row(name: "not_registered", input: Banner.input(.notRegistered)),
            Row(
                name: "not_registered, connection_refused, last known",
                input: Banner.input(
                    .notRegistered, lastKnownAt: FixtureClock.minutesAgo(5),
                    code: "ipc.connection_refused")),
            Row(name: "not_registered, refreshing after unreachable", input: refreshing),
            Row(name: "requires_approval", input: Banner.input(.requiresApproval)),
            Row(name: "disabled", input: Banner.input(.disabled)),
            Row(
                name: "enabled, 14 seconds",
                input: Banner.input(.enabled, since: FixtureClock.secondsAgo(14))),
            Row(
                name: "enabled, 15 seconds",
                input: Banner.input(.enabled, since: FixtureClock.secondsAgo(15))),
            Row(
                name: "enabled, 15 seconds, starting",
                input: Banner.input(
                    .enabled, since: FixtureClock.secondsAgo(15), isStarting: true)),
            Row(name: "not_found", input: Banner.input(.notFound)),
            Row(
                name: "unavailable, helper_missing",
                input: Banner.input(.unavailable, reason: .helperMissing)),
            Row(
                name: "unavailable, not_bundled",
                input: Banner.input(.unavailable, reason: .notBundled)),
            Row(
                name: "unavailable, root_override",
                input: Banner.input(.unavailable, reason: .rootOverride)),
            Row(
                name: "unavailable, status_failed",
                input: Banner.input(.unavailable, reason: .statusFailed, detail: "timed out")),
            Row(
                name: "unavailable, status_unrecognized",
                input: Banner.input(
                    .unavailable, reason: .statusUnrecognized, detail: "unknown status 7")),
            Row(
                name: "unavailable, unsupported_os",
                input: Banner.input(.unavailable, reason: .unsupportedOs)),
        ]
    }()

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Self.rows) { row in
                VStack(alignment: .leading, spacing: 0) {
                    Text(row.name)
                        .font(.caption.monospaced())
                        .padding(.horizontal, Spacing.s16)
                        .padding(.top, Spacing.s8)
                    if let banner = AgentBanner.content(row.input, now: FixtureClock.now) {
                        DaemonBanner(
                            banner, isReconnecting: false, reconnect: {}, openDiagnostics: {})
                    } else {
                        Text("no banner")
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                            .padding(Spacing.s16)
                    }
                }
                Divider()
            }
        }
    }
}
