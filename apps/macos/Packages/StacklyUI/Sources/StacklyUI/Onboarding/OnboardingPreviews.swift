#if DEBUG
    import SwiftUI

    /// One step of the onboarding over the fixture runtime of the `onboarding` scenario, for the
    /// previews: light, dark, and the smallest window, 900 × 600 pt (spec 0016, step 7).
    private struct OnboardingPreviewHost: View {
        @State private var runtime: ShellRuntime

        init(step: OnboardingStep) {
            let runtime = ShellBootstrap.fixture(FixtureScenario(.onboarding))
            runtime.onboarding.go(to: step)
            _runtime = State(initialValue: runtime)
        }

        var body: some View {
            OnboardingFlow(onboarding: runtime.onboarding)
                .environment(runtime.model)
                .environment(runtime.commandLineTool)
                .environment(\.fixtureNow, FixtureClock.now)
        }
    }

    private let large = CGSize(width: 1280, height: 800)
    private let smallest = AppShell.minimumSize

    #Preview("Welcome, light") {
        OnboardingPreviewHost(step: .welcome).frame(width: large.width, height: large.height)
            .preferredColorScheme(.light)
    }

    #Preview("Welcome, dark") {
        OnboardingPreviewHost(step: .welcome).frame(width: large.width, height: large.height)
            .preferredColorScheme(.dark)
    }

    #Preview("Welcome, 900 × 600") {
        OnboardingPreviewHost(step: .welcome).frame(width: smallest.width, height: smallest.height)
    }

    #Preview("Background Agent, light") {
        OnboardingPreviewHost(step: .backgroundAgent)
            .frame(width: large.width, height: large.height)
            .preferredColorScheme(.light)
    }

    #Preview("Background Agent, dark") {
        OnboardingPreviewHost(step: .backgroundAgent)
            .frame(width: large.width, height: large.height)
            .preferredColorScheme(.dark)
    }

    #Preview("Background Agent, 900 × 600") {
        OnboardingPreviewHost(step: .backgroundAgent)
            .frame(width: smallest.width, height: smallest.height)
    }

    #Preview("Storage, light") {
        OnboardingPreviewHost(step: .storage).frame(width: large.width, height: large.height)
            .preferredColorScheme(.light)
    }

    #Preview("Storage, dark") {
        OnboardingPreviewHost(step: .storage).frame(width: large.width, height: large.height)
            .preferredColorScheme(.dark)
    }

    #Preview("Storage, 900 × 600") {
        OnboardingPreviewHost(step: .storage).frame(width: smallest.width, height: smallest.height)
    }

    #Preview("Catalog Trust, light") {
        OnboardingPreviewHost(step: .catalogTrust)
            .frame(width: large.width, height: large.height)
            .preferredColorScheme(.light)
    }

    #Preview("Catalog Trust, dark") {
        OnboardingPreviewHost(step: .catalogTrust)
            .frame(width: large.width, height: large.height)
            .preferredColorScheme(.dark)
    }

    #Preview("Catalog Trust, 900 × 600") {
        OnboardingPreviewHost(step: .catalogTrust)
            .frame(width: smallest.width, height: smallest.height)
    }

    #Preview("Command Line, light") {
        OnboardingPreviewHost(step: .commandLine)
            .frame(width: large.width, height: large.height)
            .preferredColorScheme(.light)
    }

    #Preview("Command Line, dark") {
        OnboardingPreviewHost(step: .commandLine)
            .frame(width: large.width, height: large.height)
            .preferredColorScheme(.dark)
    }

    #Preview("Command Line, 900 × 600") {
        OnboardingPreviewHost(step: .commandLine)
            .frame(width: smallest.width, height: smallest.height)
    }
#endif
