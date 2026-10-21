//! `MacosBackgroundAgent` from a process outside the Stackly app bundle, the
//! only case a test may run: a cargo test binary is not inside a bundle, so
//! every call answers `NotBundled` before `SMAppService` is asked, and
//! nothing is ever registered (spec 0015). Real registration runs only in
//! the manual native steps the user approves.
#![cfg(target_os = "macos")]

use stackly_platform::{AgentProbe, BackgroundAgent, CapabilityStatus, PlatformCapability};
use stackly_platform_macos::{MacosBackgroundAgent, MacosPlatform};
use stackly_testkit::TestRoot;

/// The canonical path of this test binary, as the CLI's `main` passes it.
fn test_executable() -> std::path::PathBuf {
    std::env::current_exe()
        .expect("the test binary has a path")
        .canonicalize()
        .expect("the test binary path resolves")
}

fn assert_not_bundled(probe: AgentProbe) {
    match probe {
        AgentProbe::NotBundled { executable } => {
            assert!(executable.is_absolute(), "{}", executable.display());
        }
        other => panic!("expected NotBundled, got {other:?}"),
    }
}

#[test]
fn status_outside_the_bundle_is_not_bundled() {
    let agent = MacosBackgroundAgent::new(test_executable());
    assert_not_bundled(agent.status().expect("status answers"));
}

#[test]
fn every_change_outside_the_bundle_answers_not_bundled_without_the_os() {
    let agent = MacosBackgroundAgent::new(test_executable());
    assert_not_bundled(agent.register().expect("register answers"));
    assert_not_bundled(agent.unregister().expect("unregister answers"));
    assert_not_bundled(agent.relaunch().expect("relaunch answers"));
}

#[test]
fn an_unlocated_agent_answers_not_bundled_with_an_empty_executable() {
    let agent = MacosBackgroundAgent::unlocated();
    for probe in [
        agent.status(),
        agent.register(),
        agent.unregister(),
        agent.relaunch(),
    ] {
        assert_eq!(
            probe.expect("the call answers"),
            AgentProbe::NotBundled {
                executable: std::path::PathBuf::new()
            }
        );
    }
}

#[test]
fn macos_platform_installs_the_unlocated_agent_and_reports_it_supported() {
    let platform = MacosPlatform::with_roots(TestRoot::new().expect("test root").roots());
    assert_eq!(
        platform
            .info()
            .capabilities()
            .status(PlatformCapability::BackgroundAgent),
        CapabilityStatus::Supported
    );
    assert_eq!(
        platform
            .background_agent()
            .status()
            .expect("status answers"),
        AgentProbe::NotBundled {
            executable: std::path::PathBuf::new()
        }
    );
}
