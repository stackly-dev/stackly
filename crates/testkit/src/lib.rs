//! Fakes and fixtures for Stackly tests.
//!
//! Add this crate only as a dev dependency. It never depends on
//! `stackly-core`, so core unit tests see a single copy of every type.
//!
//! Integration tests take their own [`TestRoot`] and ports from
//! [`allocate_port`], so they never touch the real per user folders, never
//! share state, and can run in parallel. Use cases run against
//! [`FakePlatform`], whose [`FakeFilesystem`] works on the real disk but
//! fails any call that leaves the test root.
//!
//! Tests that prove a PRD ID write evidence through [`evidence`], the
//! re-exported `stackly-evidence` crate (spec 0002).
//!
//! Core and daemon tests that need a registry use [`FakeRegistry`], set up
//! records with the fixture builders ([`RuntimeBuilder`],
//! [`InstanceBuilder`], and the rest), and every `Registry` implementation
//! runs [`registry_conformance`] (spec 0005).
//!
//! Tests that need a process to supervise run the dummy server from
//! [`fake_server::binary`], which builds `stackly-fake-server` once per
//! test process (spec 0009).
//!
//! The fakes need Unix file ownership and modes, and the dummy server
//! handles Unix signals, so they exist only on Unix; [`TestRoot`] and
//! [`allocate_port`] work everywhere.

#[cfg(unix)]
mod conformance;
#[cfg(unix)]
mod fake_fs;
#[cfg(unix)]
mod fake_platform;
#[cfg(unix)]
pub mod fake_server;
#[cfg(unix)]
mod fifo;
mod port;
#[cfg(unix)]
mod registry;
mod test_root;

#[cfg(unix)]
pub use conformance::fs_conformance;
#[cfg(unix)]
pub use fake_fs::{FakeFilesystem, Fault, FsOp, Mutation};
#[cfg(unix)]
pub use fake_platform::{FakeClock, FakeIdentity, FakePlatform, FakePlatformInfo};
#[cfg(unix)]
pub use fifo::make_fifo;
pub use port::allocate_port;
#[cfg(unix)]
pub use registry::{
    BackupSeed, EnvironmentBuilder, FakeRegistry, InstanceBuilder, LeaseBuilder, OperationBuilder,
    ProviderRevisionBuilder, RegistrySeed, RuntimeBuilder, SecretReferenceSeed, WrittenAt,
    registry_conformance,
};
pub use stackly_evidence as evidence;
pub use test_root::TestRoot;
