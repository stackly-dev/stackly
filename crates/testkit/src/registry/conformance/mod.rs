//! [`registry_conformance`]: one set of scenarios that every `Registry`
//! implementation passes, so the fake and the SQLite registry cannot drift
//! apart unnoticed (spec 0005, AC-8 to AC-11, AC-14, AC-16).

mod journal;
mod prune;
mod races;
mod reads;
mod secrets;
mod writes;

use std::fmt::{Debug, Display};

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    BatchError, Committed, Generation, Instance, InstanceId, ProviderRevision,
    ProviderRevisionRecord, Registry, RegistryError, ResourceRevision, Runtime, RuntimeId,
    Versioned, WriteBatch, WriteOp,
};
use stackly_platform::Clock;

use super::seed::RegistrySeed;
use super::stored_instance;
use super::{InstanceBuilder, ProviderRevisionBuilder, RuntimeBuilder};
use crate::FakeClock;

/// Runs the same registry scenarios against any [`Registry`] and the
/// [`RegistrySeed`] that writes into it, and panics with the name of the
/// first check that fails.
///
/// It covers the commit check order, compare and swap on revisions, the
/// generation, instance names and short keys, the stored instance paths,
/// runtime references, leases and dedicated owners, round trips of every
/// aggregate and state code, operation pages, the operation journal tables
/// of spec 0007 (round trips, the check order of the journal ops and of
/// `IdempotencyKeyTaken`, upserts that keep their children, unique rules,
/// the four journal reads, and prune with cascade), the secret reference,
/// use, and audit ops and reads of feature 11 (spec 0005 "Tambahan fitur
/// 11"), retention, and three races of 16 threads (same name, same
/// artifact, same revision).
///
/// # What the caller provides
///
/// - `registry` and `seed` reach the same registry. It may hold data
///   already: every scenario works on records it creates with new IDs.
///   The prune scenario runs last and deletes every finished operation,
///   audit event, released lease, and process observation in it.
/// - `clock` is the clock the registry stamps rows with, and it still reads
///   the time the registry was opened (for [`FakeRegistry`], its
///   `loaded_at`): instances must read back observed at `clock.now()` as
///   it is when the suite starts. The suite moves the clock forward by 400
///   days before the prune scenario.
/// - Nothing else writes to the registry during the call.
///
/// ```
/// use std::sync::Arc;
///
/// use stackly_platform::Clock;
/// use stackly_testkit::{FakeClock, FakeRegistry, registry_conformance};
///
/// let clock = FakeClock::new();
/// let registry = FakeRegistry::new(Arc::new(clock.clone()), clock.now());
/// registry_conformance(&registry, &registry, &clock);
/// ```
///
/// [`FakeRegistry`]: crate::FakeRegistry
///
/// # Panics
///
/// Panics with `registry_conformance check failed: <check>: <detail>` at
/// the first check that fails.
pub fn registry_conformance(registry: &dyn Registry, seed: &dyn RegistrySeed, clock: &FakeClock) {
    let suite = Suite {
        registry,
        seed,
        clock,
        loaded_at: clock.now(),
    };
    writes::generation(&suite);
    writes::check_order(&suite);
    writes::compare_and_swap(&suite);
    writes::inserts(&suite);
    writes::runtimes(&suite);
    writes::names_and_short_keys(&suite);
    writes::software(&suite);
    writes::environments(&suite);
    writes::leases(&suite);
    reads::round_trip(&suite);
    reads::lists(&suite);
    reads::stored_paths(&suite);
    reads::runtime_references(&suite);
    reads::operation_pages(&suite);
    secrets::secrets(&suite);
    journal::journal(&suite);
    races::races(&suite);
    prune::prune(&suite);
}

/// What every scenario gets.
struct Suite<'a> {
    registry: &'a dyn Registry,
    seed: &'a dyn RegistrySeed,
    clock: &'a FakeClock,
    /// The time instances must read back as observed at.
    loaded_at: Timestamp,
}

/// A stored provider revision and a stored managed runtime of the same
/// software, for instances to sit on.
struct Stack {
    provider: ProviderRevision,
    runtime: Runtime,
}

impl Suite<'_> {
    /// Stores a provider revision, an artifact, and a runtime of it.
    fn stack(&self, check: &str) -> Stack {
        let provider = ProviderRevisionBuilder::new();
        let runtime = RuntimeBuilder::new();
        let record: ProviderRevisionRecord = provider.record();
        let built = runtime.clone().build();
        self.commit(
            check,
            vec![
                WriteOp::InsertProviderRevision(record.clone()),
                WriteOp::InsertArtifact(runtime.artifact()),
                WriteOp::InsertRuntime(built.clone()),
            ],
        );
        Stack {
            provider: record.revision,
            runtime: built,
        }
    }

    /// Starts an instance of `stack`.
    fn instance(stack: &Stack) -> InstanceBuilder {
        InstanceBuilder::new(&stack.runtime, stack.provider.key())
    }

    /// Commits `ops`, which must succeed.
    fn commit(&self, check: &str, ops: Vec<WriteOp>) -> Committed {
        self.commit_batch(check, WriteBatch::new(ops))
    }

    /// Commits `batch`, which must succeed.
    fn commit_batch(&self, check: &str, batch: WriteBatch) -> Committed {
        self.registry
            .commit(batch)
            .unwrap_or_else(|err| fail(check, &err.to_string()))
    }

    /// Commits `batch`, which must fail with exactly `expected` and leave
    /// the generation where it was.
    fn rejected(&self, check: &str, batch: WriteBatch, expected: BatchError) {
        let before = self.generation();
        match self.registry.commit(batch) {
            Ok(committed) => fail(
                check,
                &format!(
                    "the commit succeeded at generation {}, expected {expected:?}",
                    committed.generation
                ),
            ),
            Err(err) => expect_eq(check, err, expected),
        }
        expect_eq(
            &format!("{check} (a failed batch leaves the generation)"),
            self.generation(),
            before,
        );
    }

    /// Returns the generation the registry is at now.
    fn generation(&self) -> Generation {
        self.read("read the generation", self.registry.known_instance_ids())
            .generation
    }

    /// Unwraps a read.
    fn read<T>(&self, check: &str, result: Result<Versioned<T>, RegistryError>) -> Versioned<T> {
        result.unwrap_or_else(|err| fail(check, &err.to_string()))
    }

    /// Unwraps a read and drops the generation.
    fn value<T>(&self, check: &str, result: Result<Versioned<T>, RegistryError>) -> T {
        self.read(check, result).value
    }

    /// Returns `instance` as the registry must read it back: readiness and
    /// health `unknown`, reason `daemon_restarted`, observed when the
    /// registry was opened.
    fn expected_instance(&self, instance: &Instance) -> Instance {
        Instance::restore(stored_instance(instance), self.loaded_at)
            .unwrap_or_else(|err| fail("rebuild an expected instance", &err.to_string()))
    }
}

/// A batch of `ops` that does not depend on the generation.
fn batch(ops: Vec<WriteOp>) -> WriteBatch {
    WriteBatch::new(ops)
}

/// The error of the op at `index`.
const fn at(index: usize, error: RegistryError) -> BatchError {
    BatchError::at(index, error)
}

/// A revision number from the suite's own constants.
fn rev(number: u64) -> ResourceRevision {
    ResourceRevision::new(number).unwrap_or_else(|err| fail("make a revision", &err.to_string()))
}

/// `at` moved by `seconds`, forward or back.
fn shifted(at: Timestamp, seconds: i64) -> Timestamp {
    at.checked_add(SignedDuration::from_secs(seconds))
        .unwrap_or_else(|err| fail("shift a time", &err.to_string()))
}

/// Seconds in one day.
const DAY: i64 = 24 * 60 * 60;

/// An instance ID that shares its short key (the last 8 hex characters)
/// with `id`.
fn same_short_key(id: InstanceId) -> InstanceId {
    let fresh = InstanceId::generate().to_string();
    let text = id.to_string();
    format!("{}{}", &fresh[..28], &text[28..])
        .parse()
        .unwrap_or_else(|err: stackly_domain::InvalidId| fail("make a twin ID", &err.to_string()))
}

/// A new external runtime folder no other runtime uses.
fn fresh_root() -> String {
    format!("/conformance/external/{}", RuntimeId::generate())
}

/// The last 8 hex characters of an ID, for names that must not repeat.
fn suffix(id: impl Display) -> String {
    let text = id.to_string();
    text[text.len() - 8..].to_owned()
}

/// Unwraps a domain call the suite expects to succeed.
fn domain<T, E: Display>(check: &str, result: Result<T, E>) -> T {
    result.unwrap_or_else(|err| fail(check, &err.to_string()))
}

/// Parses text the suite promises is valid.
fn parse<T>(check: &str, text: &str) -> T
where
    T: std::str::FromStr,
    T::Err: Display,
{
    text.parse()
        .unwrap_or_else(|err: T::Err| fail(check, &format!("{text:?}: {err}")))
}

fn expect_eq<T: PartialEq + Debug>(check: &str, actual: T, expected: T) {
    if actual != expected {
        fail(check, &format!("expected {expected:?}, got {actual:?}"));
    }
}

fn expect(check: &str, condition: bool, detail: impl FnOnce() -> String) {
    if !condition {
        fail(check, &detail());
    }
}

fn fail(check: &str, detail: &str) -> ! {
    panic!("registry_conformance check failed: {check}: {detail}")
}
