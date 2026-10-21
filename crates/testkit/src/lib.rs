//! Fakes and fixtures for Stackly tests.
//!
//! Add this crate only as a dev dependency. It never depends on
//! `stackly-core`, so core unit tests see a single copy of every type.
//!
//! Integration tests take their own [`TestRoot`] and ports from
//! [`allocate_port`], so they never touch the real per user folders, never
//! share state, and can run in parallel. Use cases run against
//! [`FakePlatform`], whose [`FakeFilesystem`] works on the real disk but
//! fails any call that leaves the test root, and whose [`FakeSecretStore`]
//! keeps secrets in memory and can be locked, denied, held, or failed
//! (spec 0011).
//!
//! Tests that prove a PRD ID write evidence through [`evidence`], the
//! re-exported `stackly-evidence` crate (spec 0002).
//!
//! Core and daemon tests that need a registry use [`FakeRegistry`], set up
//! records with the fixture builders ([`RuntimeBuilder`],
//! [`InstanceBuilder`], and the rest), and every `Registry` implementation
//! runs [`registry_conformance`] (spec 0005).
//!
//! Tests of the daemon run the real `stacklyd` through [`SpawnedDaemon`],
//! which never lets the process outlive the test, and tests of another
//! crate find the binary through [`cargo_bin`] (spec 0006), and an example
//! through [`cargo_example`] (spec 0014).
//!
//! End to end tests of the daemon and the CLI count and reap processes
//! through [`ProcessTable`] and [`ReapGuard`], and run the `stackly` binary
//! on a test root through [`CliRunner`] (spec 0014).
//!
//! Tests that need a process to supervise run the dummy server from
//! [`fake_server::binary`], which builds `stackly-fake-server` once per
//! test process (spec 0009).
//!
//! Tests of the process supervisor run against [`FakeProcessSupervision`],
//! the process port of [`FakePlatform`]: made up processes the test starts,
//! changes, and ends, real lock and spool files in the test root, a call
//! log, and injectable errors (spec 0010).
//!
//! Tests of the runtime download run against [`FakeFetcher`], the download
//! port of [`FakePlatform`]: bytes served from memory, a script per URL for
//! interrupted, ignored-range, and failing downloads, and a record of every
//! request (spec 0018).
//!
//! Tests of install validation run against [`FakeCodeSigning`], the code
//! signing port of [`FakePlatform`]: the signature and quarantine of each
//! file are scripted by path suffix, and every call is logged (spec 0018).
//!
//! On macOS, [`DiskImage`] attaches a small disk image inside a test root,
//! so [`fs_cross_device_conformance`] can check that a removal never
//! crosses onto another device (spec 0007).
//!
//! The fakes need Unix file ownership and modes, and the dummy server
//! handles Unix signals, so they exist only on Unix; [`TestRoot`] and
//! [`allocate_port`] work everywhere.

mod cargo_bin;
mod cargo_example;
#[cfg(unix)]
mod cli_runner;
#[cfg(unix)]
mod conformance;
#[cfg(target_os = "macos")]
mod disk_image;
mod fake_agent;
mod fake_codesign;
mod fake_fetcher;
#[cfg(unix)]
mod fake_fs;
#[cfg(unix)]
mod fake_platform;
#[cfg(unix)]
mod fake_process;
mod fake_secret;
#[cfg(unix)]
pub mod fake_server;
#[cfg(unix)]
mod fifo;
mod port;
#[cfg(unix)]
mod process_table;
#[cfg(unix)]
mod reap_guard;
#[cfg(unix)]
mod registry;
#[cfg(unix)]
mod spawned_daemon;
mod test_root;

pub use cargo_bin::cargo_bin;
pub use cargo_example::cargo_example;
#[cfg(unix)]
pub use cli_runner::{CLI_TIMEOUT, CliOutput, CliRunner};
#[cfg(unix)]
pub use conformance::fs_conformance;
#[cfg(target_os = "macos")]
pub use conformance::{CrossDevice, fs_cross_device_conformance};
#[cfg(target_os = "macos")]
pub use disk_image::DiskImage;
pub use fake_agent::{FAKE_AGENT_BUNDLE, FakeBackgroundAgent};
pub use fake_codesign::{FakeCodeSigning, SigningCall};
pub use fake_fetcher::{FakeFetcher, FakeRequest, FakeStep};
#[cfg(unix)]
pub use fake_fs::{FakeFilesystem, FakeVolume, Fault, FsOp, Mutation};
#[cfg(unix)]
pub use fake_platform::{FakeClock, FakeIdentity, FakePlatform, FakePlatformInfo};
#[cfg(unix)]
pub use fake_process::{
    FakeProcessState, FakeProcessSupervision, FakeSpawn, ProcessCall, ProcessOp,
};
pub use fake_secret::{FakeSecretStore, SecretCall, SecretHold, SecretOp};
#[cfg(unix)]
pub use fifo::make_fifo;
pub use port::allocate_port;
#[cfg(unix)]
pub use process_table::{ProcessEntry, ProcessTable};
#[cfg(unix)]
pub use reap_guard::{REAP_GRACE, ReapGuard};
#[cfg(unix)]
pub use registry::{
    BackupSeed, EnvironmentBuilder, FakeRegistry, InstanceBuilder, LeaseBuilder, OperationBuilder,
    ProviderRevisionBuilder, RegistrySeed, RuntimeBuilder, SEEDED_EXECUTABLE, WrittenAt,
    registry_conformance, seeded_observation,
};
#[cfg(unix)]
pub use spawned_daemon::{
    DAEMON_START_TIMEOUT, DAEMON_STOP_TIMEOUT, EMPTY_CATALOG_JSON, SpawnCatalog, SpawnedDaemon,
};
pub use stackly_evidence as evidence;
pub use test_root::TestRoot;
