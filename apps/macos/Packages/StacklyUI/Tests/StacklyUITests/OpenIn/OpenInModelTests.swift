import AppKit
import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization
import Testing

@testable import StacklyUI

/// The Open In menu and what picking an action does (spec 0070 AC-5 to AC-11): which actions
/// show, which are on and why not, the copy of the password before the open, the cards, and
/// that a password never leaves `secret.reveal` and the concealed pasteboard.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct OpenInModelTests {
    /// One case of `detail-open-actions.json`.
    private struct ActionCase: Decodable {
        let action: OpenAction
        let label: String
    }

    private struct ActionFile: Decodable {
        let cases: [ActionCase]
    }

    /// Records the `(instance, action)` pairs `instance.prepare_terminal` was asked for.
    private nonisolated final class Prepared: Sendable {
        private let asked = Mutex<[String]>([])
        private let answer: @Sendable (String) -> Result<String, ClientError>

        init(_ answer: @escaping @Sendable (String) -> Result<String, ClientError>) {
            self.answer = answer
        }

        var actions: [String] { asked.withLock { $0 } }

        var call: @Sendable (UUID, String) async throws(ClientError) -> String {
            { [self] _, action throws(ClientError) in
                asked.withLock { $0.append(action) }
                return try answer(action).get()
            }
        }
    }

    private nonisolated static let script =
        "/Users/stackly-fixture/instances/temp/open-psql.command"

    private let fixture = FixtureOpenIn()
    private let pasteboard = FakePasteboard()
    private let sleeper = ManualSleep()
    private let credential = FixtureCredential()
    private let postgres = FixtureScenario.instanceCases[0]

    private var concealed: ConcealedPasteboard {
        let sleeper = sleeper
        return ConcealedPasteboard(
            pasteboard: pasteboard, sleep: { try await sleeper.sleep(for: $0) })
    }

    private func secrets(failing: String? = nil) -> SecretsScript {
        let value = credential.value
        return SecretsScript { call in
            guard case .secretReveal(let params) = call else {
                return .failure(SecretsScript.rejection("request.unknown_method"))
            }
            if let failing {
                return .failure(SecretsScript.rejection(failing, category: .unavailable))
            }
            return .success(SecretsScript.revealReply(params.reference, value))
        }
    }

    private func model(
        _ script: SecretsScript, prepared: Prepared = Prepared { _ in .success(Self.script) }
    ) -> OpenInModel {
        model(script.client, prepared: prepared)
    }

    private func model(
        _ secrets: SecretsClient, prepared: Prepared = Prepared { _ in .success(Self.script) }
    ) -> OpenInModel {
        let ports = fixture.ports
        return OpenInModel(
            locator: ports.locator, opener: ports.opener, fileExists: ports.fileExists,
            isRegularFile: ports.isRegularFile, prepareTerminal: prepared.call, secrets: secrets,
            pasteboard: concealed)
    }

    /// A `secret.reveal` that waits until the test lets it answer, like a daemon behind a
    /// Keychain dialog.
    private final class RevealGate: Sendable {
        private let continuation = Mutex<CheckedContinuation<Void, Never>?>(nil)

        func client(value: String) -> SecretsClient {
            SecretsClient { [self] call, _ throws(ClientError) in
                guard case .secretReveal(let params) = call else {
                    throw SecretsScript.rejection("request.unknown_method")
                }
                await withCheckedContinuation { waiting in
                    continuation.withLock { $0 = waiting }
                }
                return SecretsScript.revealReply(params.reference, value)
            }
        }

        func waitUntilWaiting() async {
            while continuation.withLock({ $0 == nil }) { await Task.yield() }
        }

        func open() {
            let waiting = continuation.withLock { current in
                defer { current = nil }
                return current
            }
            waiting?.resume()
        }
    }

    private func actions() throws -> [ActionCase] {
        let url = SourceTree.url(of: "protocols/fixtures/instance/detail-open-actions.json")
        return try StacklyJSON.decoder().decode(ActionFile.self, from: Data(contentsOf: url)).cases
    }

    private func detail(
        for summary: InstanceSummary, actions: [OpenAction]? = nil,
        passwordStatus: SecretStatus = .present
    ) -> InstanceDetail {
        let base = FixtureDetail.detail(of: summary)
        return InstanceDetail(
            instanceId: base.instanceId, revision: base.revision, provider: base.provider,
            runtime: base.runtime, dataPath: base.dataPath, startup: base.startup,
            secrets: base.secrets.map {
                DetailSecret(
                    field: $0.field, title: $0.title, reference: $0.reference,
                    status: passwordStatus)
            }, dependents: base.dependents, connections: base.connections,
            openActions: actions ?? base.openActions)
    }

    private func item(_ named: String, in content: OpenInContent) throws -> StacklyAction {
        guard case .sections(let sections) = content else {
            throw Failure("the menu shows no sections")
        }
        return try #require(sections.flatMap { $0 }.first { $0.id == "openIn.\(named)" })
    }

    private func titles(_ content: OpenInContent) -> [[String]] {
        guard case .sections(let sections) = content else { return [] }
        return sections.map { $0.map(\.title) }
    }

    private struct Failure: Error {
        let message: String
        init(_ message: String) { self.message = message }
    }

    // MARK: - Labels and the list

    @Test func labelsFollowTheSharedContractFixture() throws {
        for item in try actions() {
            #expect(OpenActionLabel.text(for: item.action) == item.label)
        }
    }

    @Test func aProviderActionShowsInManifestOrderAndAnAppThatIsNotInstalledDoesNot() {
        let model = model(secrets())
        let content = model.content(
            summary: postgres, detail: detail(for: postgres), report: { _ in })

        // The fake machine has TablePlus and Terminal but not Postico.
        #expect(titles(content) == [["Open in TablePlus", "Open psql in Terminal"]])
    }

    @Test func aRevealActionSitsInASectionOfItsOwnAfterTheOthers() throws {
        let cases = try actions()
        let kinds = cases.map(\.action).filter {
            $0.state == .available && [.terminal, .browser, .reveal].contains($0.kind)
        }
        let model = model(secrets())
        let content = model.content(
            summary: postgres, detail: detail(for: postgres, actions: kinds), report: { _ in })

        #expect(
            titles(content) == [
                ["Open psql in Terminal", "Open Monitoring in Browser"],
                ["Reveal Routes File in Finder"],
            ])
    }

    @Test func theMenuIsHiddenWhenNoActionShows() {
        fixture.bundles = []
        let appsOnly = (detail(for: postgres).openActions ?? []).filter { $0.kind == .app }
        let model = model(secrets())

        let content = model.content(
            summary: postgres, detail: detail(for: postgres, actions: appsOnly), report: { _ in })
        let none = model.content(
            summary: postgres, detail: detail(for: postgres, actions: []), report: { _ in })

        #expect(isHidden(content))
        #expect(isHidden(none))
    }

    @Test func theMenuIsLoadingWhileTheDetailLoadsAndDisabledWithoutASelection() {
        let model = model(secrets())
        let other = FixtureScenario.instanceCases[1]

        let waiting = model.content(summary: postgres, detail: nil, report: { _ in })
        let stale = model.content(
            summary: postgres, detail: detail(for: other), report: { _ in })
        let none = model.content(summary: nil, detail: nil, report: { _ in })

        #expect(isLoading(waiting))
        #expect(isLoading(stale))
        #expect(isNoSelection(none))
    }

    // MARK: - The gate

    @Test func aRunningReadyInstanceTurnsEveryActionOnAndStoppedOrNotReadyTurnsThemOff() throws {
        let model = model(secrets())
        let stopped = postgres.testing(
            lifecycle: .stopped, readiness: .fixture(.notReady, reason: "stopped"))
        let starting = postgres.testing(readiness: .fixture(.notReady, reason: "awaiting_probe"))

        let ready = model.content(
            summary: postgres, detail: detail(for: postgres), report: { _ in })
        let off = model.content(summary: stopped, detail: detail(for: stopped), report: { _ in })
        let waiting = model.content(
            summary: starting, detail: detail(for: starting), report: { _ in })

        #expect(try item("tableplus", in: ready).disabledReason == nil)
        #expect(try item("psql", in: ready).disabledReason == nil)
        #expect(try item("psql", in: off).disabledReason == "Start the instance to open it.")
        #expect(try item("tableplus", in: off).disabledReason == "Start the instance to open it.")
        #expect(try item("psql", in: waiting).disabledReason == "Not accepting connections yet.")
    }

    @Test func aRevealActionIsOnWhenStoppedAndOffWhenTheFileDoesNotExist() throws {
        let reveal = try #require(
            try actions().map(\.action).first { $0.kind == .reveal })
        let path = try #require(reveal.path)
        let stopped = postgres.testing(lifecycle: .stopped)
        let model = model(secrets())

        let missing = model.content(
            summary: stopped, detail: detail(for: stopped, actions: [reveal]), report: { _ in })
        fixture.existingPaths = [path]
        let present = model.content(
            summary: stopped, detail: detail(for: stopped, actions: [reveal]), report: { _ in })

        #expect(try item("routes", in: missing).disabledReason == "The file doesn’t exist yet.")
        #expect(try item("routes", in: present).disabledReason == nil)
    }

    @Test func aReasonOfTheDaemonWinsOverTheLifecycleAndAnUnknownKindSaysUpdate() throws {
        let unavailable = try actions().map(\.action).filter { $0.state == .unavailable }
        let unknown = try #require(try actions().map(\.action).first { $0.kind == .unrecognized })
        let stopped = postgres.testing(lifecycle: .stopped)
        let model = model(secrets())

        let content = model.content(
            summary: stopped,
            detail: detail(for: stopped, actions: unavailable + [unknown]), report: { _ in })

        #expect(
            try item("monitoring", in: content).disabledReason
                == "Start the instance to assign its port.")
        #expect(try item("psql", in: content).disabledReason == "This instance was removed.")
        #expect(
            try item("future", in: content).disabledReason == "Update Stackly to use this action.")
    }

    @Test func everyActionIsMarkedAsOneTheMutationGateTurnsOff() throws {
        let model = model(secrets())
        let content = model.content(
            summary: postgres, detail: detail(for: postgres), report: { _ in })

        #expect(try item("tableplus", in: content).mutates)
        #expect(try item("psql", in: content).mutates)
    }

    // MARK: - Opening an app

    @Test func anAppCopiesThePasswordOnceThenOpensTheUrlWithTheFirstInstalledApp() async throws {
        let script = secrets()
        let model = model(script)
        let summary = postgres
        let detail = detail(for: summary)
        let action = try #require(detail.openActions?.first { $0.name == "tableplus" })

        let feedback = await model.open(action, summary: summary, detail: detail)

        let reference = try #require(action.copySecret?.reference)
        #expect(
            script.made == [.secretReveal(RevealSecretParams(reference: reference, purpose: .copy))]
        )
        #expect(pasteboard.holds(credential.value))
        let text = try #require(action.url)
        let url = try #require(URL(string: text))
        #expect(
            fixture.opened == [
                FixtureOpenIn.Opened(
                    url: url,
                    application: FixtureOpenIn.applications.appending(
                        path: "com.tinyapp.TablePlus.app"))
            ])
        #expect(feedback?.tone == .positive)
        #expect(feedback?.message == "Opening “pg17-dev” in TablePlus.")
        #expect(feedback?.detail == "Password copied. Paste it when TablePlus asks.")
        #expect(url.absoluteString.contains(credential.value) == false)
    }

    @Test func aPasswordThatArrivesAfterTheClipboardChangedLeavesItAlone() async throws {
        let gate = RevealGate()
        let model = model(gate.client(value: credential.value))
        let detail = detail(for: postgres)
        let action = try #require(detail.openActions?.first { $0.name == "tableplus" })

        let opening = Task { await model.open(action, summary: postgres, detail: detail) }
        await gate.waitUntilWaiting()
        // The person copies something else while the Keychain dialog is still open.
        _ = pasteboard.clearContents()
        _ = pasteboard.setString("something else", forType: .string)
        gate.open()
        let feedback = try #require(await opening.value)

        #expect(pasteboard.strings[.string] == "something else")
        #expect(pasteboard.holds(credential.value) == false)
        #expect(fixture.opened.count == 1)
        #expect(feedback.tone == .caution)
        #expect(feedback.message == "Opened without copying the password.")
        #expect(feedback.detail == "Not copied because the clipboard changed.")
    }

    @Test func anAppFallsBackToTheNextBundleIDWhenTheFirstIsNotInstalled() async throws {
        fixture.bundles = ["at.eggerapps.Postico"]
        let model = model(secrets())
        let detail = detail(for: postgres)
        let action = try #require(detail.openActions?.first { $0.name == "postico" })

        _ = await model.open(action, summary: postgres, detail: detail)

        #expect(
            fixture.opened.map(\.application)
                == [FixtureOpenIn.applications.appending(path: "at.eggerapps.Postico.app")])
    }

    @Test func aMissingPasswordStillOpensTheAppAndOffersEnterSecret() async throws {
        let script = secrets()
        let model = model(script)
        let detail = detail(for: postgres, passwordStatus: .missing)
        let action = try #require(detail.openActions?.first { $0.name == "tableplus" })
        var needs: [SecretNeed] = []

        let feedback = try #require(
            await model.open(
                action, summary: postgres, detail: detail, enterSecret: { needs.append($0) }))

        #expect(script.made.isEmpty)
        #expect(pasteboard.changeCount == 0)
        #expect(fixture.opened.count == 1)
        #expect(feedback.tone == .caution)
        #expect(feedback.message == "Opened without copying the password.")
        #expect(feedback.detail == "The password isn’t available.")
        let enter = try #require(feedback.action)
        #expect(enter.title == "Enter Secret…")
        enter.perform()
        #expect(needs.map(\.reference) == [try #require(action.copySecret?.reference)])
    }

    @Test func aLockedKeychainOffersEnterSecretButAnotherFailureOnlyShowsItsMessage() async throws {
        let detail = detail(for: postgres)
        let action = try #require(detail.openActions?.first { $0.name == "tableplus" })

        let locked = try #require(
            await model(secrets(failing: "secret.store_locked")).open(
                action, summary: postgres, detail: detail))
        let broken = try #require(
            await model(secrets(failing: "internal.unexpected")).open(
                action, summary: postgres, detail: detail))

        #expect(locked.tone == .caution)
        #expect(locked.action?.title == "Enter Secret…")
        #expect(broken.tone == .caution)
        #expect(broken.message == "Opened without copying the password.")
        #expect(broken.detail == "something failed")
        #expect(broken.action == nil)
        #expect(fixture.opened.count == 2)
    }

    @Test func anOpenThatFailsShowsACriticalCardAndKeepsTheCopiedPassword() async throws {
        fixture.openFailure = OpenInFailure.refused
        let model = model(secrets())
        let detail = detail(for: postgres)
        let action = try #require(detail.openActions?.first { $0.name == "tableplus" })

        let feedback = try #require(await model.open(action, summary: postgres, detail: detail))

        #expect(feedback.tone == .critical)
        #expect(feedback.message == "Couldn’t open TablePlus.")
        #expect(feedback.detail == OpenInFailure.refused.errorDescription)
        #expect(pasteboard.holds(credential.value))
    }

    @Test func anAppRemovedAfterTheMenuWasBuiltShowsACriticalCardAndCopiesNothing() async throws {
        let script = secrets()
        let model = model(script)
        let detail = detail(for: postgres)
        let action = try #require(detail.openActions?.first { $0.name == "tableplus" })
        fixture.bundles = []

        let feedback = try #require(await model.open(action, summary: postgres, detail: detail))

        #expect(feedback.message == "Couldn’t open TablePlus.")
        #expect(script.made.isEmpty)
        #expect(fixture.opened.isEmpty)
    }

    // MARK: - Opening Terminal

    @Test func aTerminalActionPreparesTheScriptThenCopiesThenOpensItWithTerminal() async throws {
        let script = secrets()
        let prepared = Prepared { _ in .success(Self.script) }
        let model = model(script, prepared: prepared)
        let detail = detail(for: postgres)
        let action = try #require(detail.openActions?.first { $0.name == "psql" })

        let feedback = try #require(await model.open(action, summary: postgres, detail: detail))

        #expect(prepared.actions == ["psql"])
        #expect(script.made.count == 1)
        #expect(
            fixture.opened == [
                FixtureOpenIn.Opened(
                    url: URL(fileURLWithPath: Self.script),
                    application: FixtureOpenIn.applications.appending(
                        path: "\(OpenInModel.terminalBundleID).app"))
            ])
        #expect(feedback.tone == .positive)
        #expect(feedback.message == "Opening psql for “pg17-dev” in Terminal.")
        #expect(feedback.detail == "Password copied. Paste it when psql asks.")
    }

    @Test func aScriptPathTheDaemonDoesNotWriteIsNeverOpened() async throws {
        let symlink = "/Users/stackly-fixture/instances/temp/open-link.command"
        fixture.notRegularFiles = [symlink]
        let paths = [
            "relative/open-psql.command", "/Users/stackly-fixture/instances/temp/psql.command",
            "/Users/stackly-fixture/instances/temp/open-psql.sh",
            "/Users/stackly-fixture/instances/temp/open-.command", "/Applications/Calculator.app",
            symlink,
        ]
        for path in paths {
            let script = secrets()
            let model = model(script, prepared: Prepared { _ in .success(path) })
            let detail = detail(for: postgres)
            let action = try #require(detail.openActions?.first { $0.name == "psql" })

            let feedback = try #require(await model.open(action, summary: postgres, detail: detail))

            #expect(feedback.tone == .critical, "\(path)")
            #expect(feedback.detail == OpenInFailure.invalidTarget.errorDescription, "\(path)")
            #expect(script.made.isEmpty, "nothing is copied for \(path)")
        }
        #expect(fixture.opened.isEmpty)
    }

    @Test func theSystemCheckCountsOnlyARegularFileAndNeverASymlinkOrAFolder() throws {
        let folder = try TemporaryFolder()
        let file = folder.url.appending(path: "open-psql.command")
        try Data("#!/bin/sh\n".utf8).write(to: file)
        let link = folder.url.appending(path: "open-link.command")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)

        #expect(OpenInPorts.isRegularFile(file.path))
        #expect(OpenInPorts.isRegularFile(link.path) == false)
        #expect(OpenInPorts.isRegularFile(folder.url.path) == false)
        #expect(OpenInPorts.isRegularFile(folder.url.appending(path: "missing").path) == false)
    }

    @Test func aRefusedPrepareCopiesNothingOpensNothingAndReloadsTheDetail() async throws {
        let cases: [(code: String, detail: [ErrorDetail], text: String)] = [
            (
                "instance.open_action_unavailable",
                [ErrorDetail(key: "reason", value: "not_ready")],
                "Not accepting connections yet."
            ),
            (
                "instance.open_action_unavailable",
                [ErrorDetail(key: "reason", value: "not_running")], "Start the instance to open it."
            ),
            (
                "instance.open_action_unavailable",
                [ErrorDetail(key: "reason", value: "retained")], "This instance was removed."
            ),
            ("instance.open_action_not_found", [], "This action is no longer available."),
            ("internal.unexpected", [], "the daemon said no"),
        ]
        for refusal in cases {
            let script = secrets()
            let body = refusal
            let prepared = Prepared { _ in
                .failure(
                    SecretsScript.rejection(
                        body.code, category: .conflict, message: "the daemon said no",
                        details: body.detail))
            }
            let model = model(script, prepared: prepared)
            let detail = detail(for: postgres)
            let action = try #require(detail.openActions?.first { $0.name == "psql" })
            var reloads = 0

            let feedback = try #require(
                await model.open(
                    action, summary: postgres, detail: detail, reload: { reloads += 1 }))

            #expect(feedback.tone == .critical)
            #expect(feedback.message == "Couldn’t open psql.")
            #expect(feedback.detail == refusal.text)
            #expect(script.made.isEmpty)
            #expect(reloads == 1)
        }
        #expect(fixture.opened.isEmpty)
    }

    @Test func terminalThatCannotBeFoundShowsACriticalCard() async throws {
        fixture.bundles = []
        let script = secrets()
        let model = model(script)
        let detail = detail(for: postgres)
        let action = try #require(detail.openActions?.first { $0.name == "psql" })

        let feedback = try #require(await model.open(action, summary: postgres, detail: detail))

        #expect(feedback.message == "Couldn’t open psql.")
        #expect(script.made.isEmpty)
    }

    // MARK: - Browser and Finder

    @Test func aBrowserActionOpensTheUrlInTheDefaultBrowser() async throws {
        let nats = FixtureScenario.instanceCases[4].testing(
            lifecycle: .running, readiness: .fixture(.ready, reason: "probe_ok"))
        let detail = detail(for: nats)
        let action = try #require(detail.openActions?.first { $0.name == "monitoring" })
        let model = model(secrets())

        let feedback = try #require(await model.open(action, summary: nats, detail: detail))

        let text = try #require(action.url)
        let url = try #require(URL(string: text))
        #expect(fixture.browsed == [url])
        #expect(fixture.opened.isEmpty)
        #expect(feedback.message == "Opening Monitoring for “queue” in your browser.")
    }

    @Test func aBrowserActionWithAnotherSchemeIsNeverOpened() async throws {
        let nats = FixtureScenario.instanceCases[4].testing(
            lifecycle: .running, readiness: .fixture(.ready, reason: "probe_ok"))
        let model = model(secrets())
        for text in ["file:///etc/passwd", "javascript:alert(1)", "ssh://host", "localhost:8222"] {
            let action = OpenAction(
                name: "monitoring", title: "Monitoring", kind: .browser, state: .available,
                reason: nil, appBundleIds: nil, url: text, command: nil, path: nil,
                copySecret: nil)
            let detail = detail(for: nats, actions: [action])

            let feedback = try #require(await model.open(action, summary: nats, detail: detail))

            #expect(feedback.tone == .critical, "\(text)")
            #expect(feedback.detail == OpenInFailure.invalidTarget.errorDescription, "\(text)")
        }
        #expect(fixture.browsed.isEmpty)
    }

    @Test func anUppercaseHttpsSchemeStillOpensInTheBrowser() async throws {
        let nats = FixtureScenario.instanceCases[4].testing(
            lifecycle: .running, readiness: .fixture(.ready, reason: "probe_ok"))
        let action = OpenAction(
            name: "monitoring", title: "Monitoring", kind: .browser, state: .available,
            reason: nil, appBundleIds: nil, url: "HTTPS://127.0.0.1:8222/", command: nil,
            path: nil, copySecret: nil)

        _ = await model(secrets()).open(
            action, summary: nats, detail: detail(for: nats, actions: [action]))

        #expect(fixture.browsed.map(\.absoluteString) == ["HTTPS://127.0.0.1:8222/"])
    }

    @Test func aRevealActionShowsTheFileInFinderWithoutACard() async throws {
        let reveal = try #require(try actions().map(\.action).first { $0.kind == .reveal })
        let model = model(secrets())
        let detail = detail(for: postgres, actions: [reveal])

        let feedback = await model.open(reveal, summary: postgres, detail: detail)

        #expect(feedback == nil)
        #expect(fixture.revealed == [URL(fileURLWithPath: try #require(reveal.path))])
    }

    // MARK: - Choosing an item

    @Test func choosingAnItemOfTheMenuReportsTheCard() async throws {
        let model = model(secrets())
        var cards: [Feedback] = []
        let content = model.content(
            summary: postgres, detail: detail(for: postgres), report: { cards.append($0) })

        try item("tableplus", in: content).perform()
        while cards.isEmpty { await Task.yield() }

        #expect(cards.first?.message == "Opening “pg17-dev” in TablePlus.")
    }

    // MARK: - No secret in the model or the cards

    @Test func thePasswordIsNeverInTheModelTheCardsTheUrlsOrTheMenu() async throws {
        let model = model(secrets())
        let detail = detail(for: postgres)
        var cards: [Feedback] = []
        for name in ["tableplus", "psql"] {
            let action = try #require(detail.openActions?.first { $0.name == name })
            cards.append(try #require(await model.open(action, summary: postgres, detail: detail)))
        }
        let content = model.content(summary: postgres, detail: detail, report: { _ in })

        credential.assertAbsent(in: "\(model)", what: "the model")
        credential.assertAbsent(in: "\(cards)", what: "the cards")
        credential.assertAbsent(in: "\(titles(content))", what: "the menu")
        credential.assertAbsent(in: "\(fixture.opened)", what: "what was opened")
        credential.assertAbsent(in: "\(detail)", what: "the detail")
        #expect(pasteboard.holds(credential.value))
    }

    @Test func theDetailModelAndTheOpenInModelKeepNoPasswordAfterAnOpen() async throws {
        let detail = detail(for: postgres)
        let script = DetailScript(detail: detail, instance: postgres)
        let models = InstanceDetailModels(service: script.service, openIn: fixture.ports)
        let detailModel = models.model(for: postgres.id)
        await detailModel.open()
        let action = try #require(detailModel.detail?.openActions?.first { $0.name == "tableplus" })
        let reveal = secrets()
        let sink = DiscardingPasteboard()
        let openIn = OpenInModel(
            locator: fixture.ports.locator, opener: fixture.ports.opener,
            fileExists: fixture.ports.fileExists,
            prepareTerminal: { _, _ throws(ClientError) in Self.script }, secrets: reveal.client,
            pasteboard: ConcealedPasteboard(pasteboard: sink))

        _ = await openIn.open(action, summary: postgres, detail: try #require(detailModel.detail))

        credential.assertAbsent(in: Self.dumped(detailModel), what: "InstanceDetailModel")
        credential.assertAbsent(in: Self.dumped(openIn), what: "OpenInModel")
        credential.assertAbsent(in: Self.dumped(models.openIn), what: "the shared OpenInModel")
    }

    // MARK: - Helpers

    /// Every stored property of `value`, nested, as text: what a leak into a model would show in.
    private static func dumped(_ value: some Any) -> String {
        var text = ""
        dump(value, to: &text, maxDepth: 12)
        return text
    }

    /// A pasteboard that keeps nothing, so a dump of the model reaches no copied password.
    private final class DiscardingPasteboard: PasteboardStore {
        private(set) var changeCount = 0

        func prepareForNewContents(with options: NSPasteboard.ContentsOptions) -> Int {
            changeCount += 1
            return changeCount
        }

        func clearContents() -> Int {
            changeCount += 1
            return changeCount
        }

        func setString(_ string: String, forType dataType: NSPasteboard.PasteboardType) -> Bool {
            true
        }
    }

    private func isHidden(_ content: OpenInContent) -> Bool {
        if case .hidden = content { return true }
        return false
    }

    private func isLoading(_ content: OpenInContent) -> Bool {
        if case .loading = content { return true }
        return false
    }

    private func isNoSelection(_ content: OpenInContent) -> Bool {
        if case .noSelection = content { return true }
        return false
    }
}
