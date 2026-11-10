//! The doctor (spec 0034): the read only checks of the daemon, run on
//! demand, each with a time limit, and what they found.
//!
//! A check reads the registry, the filesystem, and the processes through
//! the platform ports and reports typed [`Finding`]s; the daemon turns them
//! into the wire report with its English text. Nothing here writes the
//! registry, creates an operation, changes a file, takes or probes the
//! instance lock, signals a process, or spawns one: the identity of a
//! recorded process is checked passively (`observe_passive`), and the only
//! sockets opened are the trial binds of the port checks of spec 0029.
//!
//! [`Doctor`] is the long lived runner: one per daemon, with its two run
//! slots and the check tasks that ran past their limit.

mod checks;
mod runner;
mod types;

pub use runner::{Doctor, DoctorLimits};
pub use types::{
    Action, CapacityFailure, CapacityOutcome, CheckReport, DaemonFacts, DegradedSubsystem,
    DiskReading, DoctorError, DoctorMode, DoctorRequest, DoctorSources, FailureSnapshot, Finding,
    FindingFacts, IdentityFacts, IdentityReason, InstanceRef, PathProblem, PortOwnerFacts,
    PortProblemKind, Report, RootProblem, SummaryFacts,
};
