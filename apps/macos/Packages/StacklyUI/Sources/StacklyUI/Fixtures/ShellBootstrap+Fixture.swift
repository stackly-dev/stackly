#if DEBUG
    import StacklyClient

    extension ShellBootstrap {
        /// Builds a shell that plays `scenario`: a ``DaemonModel`` over a ``FixtureLink`` and
        /// its signals, on the fixture clock. It never opens a socket, reads
        /// `STACKLY_ROOT`, starts a process, or writes defaults or a file (spec 0068).
        /// - Parameter scenario: The scenario to play.
        /// - Returns: The runtime, already started.
        public static func fixture(_ scenario: FixtureScenario) -> ShellRuntime {
            let link = FixtureLink(scenario)
            let agent = BackgroundAgentModel(
                controller: FakeBackgroundAgentController(
                    status: scenario.agent.status.result,
                    afterRegister: scenario.agent.afterRegister.result),
                launchRecord: FakeLaunchRecordReader(launchedAt: scenario.agent.launchedAt),
                rootOverrideActive: false,
                initialProbe: scenario.agent.status.result,
                initialLaunchRecord: scenario.agent.launchedAt.map {
                    LaunchRecordStatus(exists: true, lastLaunchedAt: $0)
                } ?? .missing)
            let model = DaemonModel(
                link: .fixture(link),
                signals: link.signals,
                endpoint: nil,
                launchedAt: FixtureClock.now,
                now: { FixtureClock.now },
                agent: agent)
            link.attach { [weak model] in
                await MainActor.run {
                    guard let model else { return true }
                    switch model.presence {
                    case .connecting, .refreshing: return false
                    default: return true
                    }
                }
            }
            let tool = FakeCommandLineTool()
            return ShellRuntime(
                model: model, closeNotice: CloseNotice(presentsPanel: false),
                commandLineTool: CommandLineToolModel(
                    controller: tool, initialStatus: FakeCommandLineTool.fixtureStatus),
                onboardingStore: InMemoryOnboardingRecordStore(
                    isFinished: scenario.onboardingFinished))
        }
    }
#endif
