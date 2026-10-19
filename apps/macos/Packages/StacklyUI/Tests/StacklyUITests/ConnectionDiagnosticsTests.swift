import Foundation
import Testing

@testable import StacklyUI

/// The Connection section of Diagnostics: hints, rows, "last known", Reconnect, and Copy
/// Details (spec 0013 AC-12).
struct ConnectionDiagnosticsTests {
    nonisolated static let now = Date(timeIntervalSince1970: 1_792_245_720)

    // MARK: - Hints

    nonisolated static let codeHints: [(String, String)] = [
        (
            "ipc.endpoint_missing",
            "stacklyd isn’t running. Start it with stacklyd, then click Reconnect."
        ),
        (
            "ipc.connection_refused",
            "A stale socket is left from a stopped stacklyd. Start stacklyd, then click Reconnect."
        ),
        ("ipc.call_timeout", "stacklyd stopped answering."),
        ("ipc.connection_closed", "stacklyd closed the connection."),
        ("daemon.shutting_down", "stacklyd is shutting down."),
        (
            "ipc.protocol_violation",
            "stacklyd sent a message Stackly couldn’t read. Check that both come from the same build."
        ),
        ("protocol.major_mismatch", "Install matching versions of Stackly and stacklyd."),
    ]

    @Test func everyKnownCodeHasItsHint() {
        let status = ShellStatus.unreachable(lastKnownAt: nil, since: Self.now)
        for (code, hint) in Self.codeHints {
            let error = DiagnosticsError(code: code, category: "unavailable", message: "m")
            #expect(ConnectionHint.text(status: status, error: error) == hint, "\(code)")
        }
    }

    @Test func aHandshakeCallTimeoutSaysFiveSecondsAndTheDaemonOnlyCodeHasNoHint() {
        let status = ShellStatus.unreachable(lastKnownAt: nil, since: Self.now)
        let handshake = DiagnosticsError(
            code: "ipc.call_timeout",
            category: "timeout",
            message: "the daemon did not answer in time",
            method: "daemon.handshake"
        )
        let call = DiagnosticsError(
            code: "ipc.call_timeout",
            category: "timeout",
            message: "the daemon did not answer in time",
            method: "daemon.get_health"
        )
        let daemonOnly = DiagnosticsError(
            code: "ipc.handshake_timeout", category: "timeout", message: "m")

        #expect(
            ConnectionHint.text(status: status, error: handshake)
                == "stacklyd didn’t answer within 5 seconds."
        )
        #expect(ConnectionHint.text(status: status, error: call) == "stacklyd stopped answering.")
        #expect(ConnectionHint.text(status: status, error: daemonOnly) == nil)
    }

    @Test func ioErrorHintNamesTheOperationAndKind() {
        let error = DiagnosticsError(
            code: "ipc.io_error",
            category: "unavailable",
            message: "an os error stopped the connection",
            operation: "connect",
            kind: "permission_denied"
        )
        let status = ShellStatus.unreachable(lastKnownAt: nil, since: Self.now)

        #expect(
            ConnectionHint.text(status: status, error: error)
                == "Stackly couldn’t reach the socket (connect: permission_denied)."
        )
    }

    nonisolated static let untrustedReasons: [(ShellStatus.UntrustedCause, String)] = [
        (.endpoint(reason: "not_a_socket"), "the path isn’t a socket"),
        (.endpoint(reason: "symlink"), "the path is a symbolic link"),
        (.endpoint(reason: "wrong_owner"), "the socket belongs to another user"),
        (.endpoint(reason: "group_or_other_access"), "other users can access the socket"),
        (
            .endpoint(reason: "parent_wrong_owner"),
            "the socket’s folder isn’t a folder owned by you"
        ),
        (
            .endpoint(reason: "parent_group_or_other_access"),
            "other users can access the socket’s folder"
        ),
        (.endpoint(reason: "peer_uid_mismatch"), "the daemon runs as another user"),
        (
            .endpoint(reason: "peer_credentials_unavailable"),
            "macOS didn’t report who runs the daemon"
        ),
        (.peerRejected, "stacklyd rejected this app’s user"),
        (.endpoint(reason: "new_reason"), "unexpected reason new_reason"),
    ]

    @Test func everyUntrustedReasonHasItsWordsAndSaysNothingWasSent() {
        for (cause, words) in Self.untrustedReasons {
            #expect(ConnectionHint.untrustedReason(cause) == words)
            let hint = ConnectionHint.text(status: .untrusted(cause, since: Self.now), error: nil)
            let sentence = words.prefix(1).uppercased() + words.dropFirst()
            #expect(hint == "\(sentence). Stackly sent nothing to this socket.")
        }
    }

    @Test func statusHintsCoverRecoveryMisconfigurationAndIncompatibility() {
        #expect(
            ConnectionHint.text(status: .recovery(since: Self.now), error: nil)
                == "See the recovery report in stacklyd’s log folder."
        )
        #expect(
            ConnectionHint.text(status: .misconfigured(.rootMissing, since: Self.now), error: nil)
                == ShellStatus.Misconfiguration.rootMissing.message
        )
        let rejected = ShellStatus.incompatible(
            .handshakeRejected(code: "request.invalid_params"),
            since: Self.now
        )
        #expect(
            ConnectionHint.text(status: rejected, error: nil)
                == "Install matching versions of Stackly and stacklyd."
        )
    }

    @Test func theDaemonsHintFollowsOursAndAnUnknownCodeHasNone() {
        let status = ShellStatus.unreachable(lastKnownAt: nil, since: Self.now)
        let withHint = DiagnosticsError(
            code: "ipc.call_timeout",
            category: "timeout",
            message: "the daemon did not answer in time",
            daemonHint: "Check the daemon log."
        )
        let unknown = DiagnosticsError(code: "ipc.other", category: "internal", message: "m")

        #expect(
            ConnectionHint.text(status: status, error: withHint)
                == "stacklyd stopped answering. Check the daemon log."
        )
        #expect(ConnectionHint.text(status: status, error: unknown) == nil)
        #expect(ConnectionHint.text(status: .connected(.ready), error: nil) == nil)
    }

    // MARK: - Rows

    nonisolated static let daemon = DaemonFacts(
        version: "0.1.0",
        pid: 4242,
        startedAt: now.addingTimeInterval(-3_600),
        serverInstanceId: "0192f4a1-7c1e-7b3a-9d2e-5f6a7b8c9d0e"
    )

    nonisolated static func rows(_ diagnostics: ConnectionDiagnostics) -> [DiagnosticsRow] {
        diagnostics.rows(
            calendar: Calendar(identifier: .gregorian),
            locale: Locale(identifier: "en_US"),
            timeZone: .gmt
        )
    }

    @Test func unreachableRowsShowTheErrorTheNextAttemptAndTheLastKnownDaemon() {
        let diagnostics = ConnectionDiagnostics(
            status: .unreachable(lastKnownAt: Self.now, since: Self.now),
            socketPath: "/var/folders/zz/abc/T/stackly/stacklyd.sock",
            lastError: DiagnosticsError(
                code: "ipc.endpoint_untrusted",
                category: "permission_denied",
                message: "the daemon socket failed a security check",
                reason: "symlink"
            ),
            nextAttempt: Self.now.addingTimeInterval(5),
            lastConnectedAt: Self.now.addingTimeInterval(-60),
            daemon: Self.daemon,
            daemonProtocol: "1.0",
            appProtocol: "1.0",
            contractDrift: true,
            health: HealthFacts(
                status: "degraded",
                subsystems: [.init(name: "registry", status: "degraded", detail: "slow disk")]
            )
        )
        let rows = Self.rows(diagnostics)

        #expect(
            rows.map(\.label) == [
                "Status", "Socket", "Last error", "What to do", "Next attempt",
                "Last connected", "stacklyd", "Protocol", "Contract", "Health",
            ]
        )
        #expect(rows[0].value == "stacklyd unavailable")
        #expect(rows[1].code == "/var/folders/zz/abc/T/stackly/stacklyd.sock")
        #expect(rows[2].code == "ipc.endpoint_untrusted")
        #expect(
            rows[2].text
                == "permission_denied · the daemon socket failed a security check · symlink"
        )
        #expect(
            rows[3].value == "The path is a symbolic link. Stackly sent nothing to this socket.")
        #expect(rows[4].value == "Oct 17, 2026 at 2:02:05\u{202F}PM")
        #expect(rows[5].value == "Oct 17, 2026 at 2:01:00\u{202F}PM")
        #expect(
            rows[6].value
                == "0.1.0, pid 4242, started Oct 17, 2026 at 1:02:00\u{202F}PM, instance 0192f4a1-7c1e-7b3a-9d2e-5f6a7b8c9d0e (last known)"
        )
        #expect(rows[7].value == "stacklyd 1.0 · Stackly 1.0")
        #expect(rows[8].value == "Different build (contract drift)")
        #expect(rows[9].value == "degraded · registry degraded: slow disk")
    }

    @Test func connectedRowsDropLastKnownAndTheNextAttempt() {
        let diagnostics = ConnectionDiagnostics(
            status: .connected(.ready),
            socketPath: "/tmp/stackly/stacklyd.sock",
            nextAttempt: Self.now,
            lastConnectedAt: Self.now,
            daemon: Self.daemon,
            daemonProtocol: "1.0",
            appProtocol: "1.0",
            contractDrift: false
        )
        let rows = Self.rows(diagnostics)

        #expect(rows.contains { $0.label == "Next attempt" } == false)
        #expect(rows.contains { $0.label == "What to do" } == false)
        #expect(rows.first { $0.label == "Last error" }?.value == "None")
        #expect(rows.first { $0.label == "stacklyd" }?.value.hasSuffix("(last known)") == false)
        #expect(rows.first { $0.label == "Contract" }?.value == "Matches")
    }

    @Test func neverConnectedRowsSayNeverAndUnavailable() {
        let diagnostics = ConnectionDiagnostics(
            status: .misconfigured(.rootEmpty, since: Self.now),
            appProtocol: nil
        )
        let rows = Self.rows(diagnostics)

        #expect(
            rows.map(\.value) == [
                "Stackly misconfigured",
                "Unavailable",
                "None",
                ShellStatus.Misconfiguration.rootEmpty.message,
                "Never",
            ]
        )
    }

    @Test func copyDetailsHoldsExactlyTheRowsShown() {
        let diagnostics = ConnectionDiagnostics(
            status: .unreachable(lastKnownAt: nil, since: Self.now),
            socketPath: "/tmp/stackly/stacklyd.sock",
            lastError: DiagnosticsError(
                code: "ipc.endpoint_missing",
                category: "unavailable",
                message: "no socket at the daemon path"
            ),
            appProtocol: "1.0"
        )
        let rows = Self.rows(diagnostics)
        let text = ConnectionDiagnostics.copyText(rows)

        #expect(text.split(separator: "\n").count == rows.count)
        #expect(
            text == """
                Status: stacklyd unavailable
                Socket: /tmp/stackly/stacklyd.sock
                Last error: ipc.endpoint_missing · unavailable · no socket at the daemon path
                What to do: stacklyd isn’t running. Start it with stacklyd, then click Reconnect.
                Last connected: Never
                Protocol: Stackly 1.0
                """
        )
    }

    @Test func noRowEverShowsTheRootOverrideValue() {
        for problem in ShellStatus.Misconfiguration.allCases {
            let diagnostics = ConnectionDiagnostics(
                status: .misconfigured(problem, since: Self.now),
                appProtocol: "1.0"
            )
            let text = ConnectionDiagnostics.copyText(Self.rows(diagnostics))
            #expect(text.contains("/Users/") == false)
            #expect(text.contains("STACKLY_ROOT=") == false)
        }
    }

    @Test func reconnectIsDisabledWithItsReasonOnlyWhileMisconfigured() {
        #expect(
            ConnectionDiagnosticsSection.reconnectDisabledReason
                == "Relaunch Stackly after fixing the problem."
        )
        let misconfigured = ShellPresentation(status: .misconfigured(.rootEmpty, since: Self.now))
        let unreachable = ShellPresentation(status: .unreachable(lastKnownAt: nil, since: Self.now))
        #expect(misconfigured.isMisconfigured)
        #expect(unreachable.isMisconfigured == false)
        #expect(misconfigured.hasConnection == false)
    }
}
