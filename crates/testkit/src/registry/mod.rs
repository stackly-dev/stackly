//! Registry fakes and fixtures (spec 0005, "Fake dan conformance"):
//! [`FakeRegistry`], the [`RegistrySeed`] trait for tables without a
//! `WriteOp`, builders for valid aggregates, and [`registry_conformance`],
//! the suite every `Registry` implementation runs.

mod builders;
mod conformance;
mod fake;
mod seed;

use stackly_domain::{Instance, StoredInstance};

pub use builders::{
    EnvironmentBuilder, InstanceBuilder, LeaseBuilder, OperationBuilder, ProviderRevisionBuilder,
    RuntimeBuilder,
};
pub use conformance::registry_conformance;
pub use fake::{FakeRegistry, WrittenAt};
pub use seed::{BackupSeed, RegistrySeed};

/// Returns every stored field of `instance`, to rebuild it the way the
/// registry does when it reads one back.
fn stored_instance(instance: &Instance) -> StoredInstance {
    StoredInstance {
        id: instance.id(),
        name: instance.name().clone(),
        display_name: instance.display_name().cloned(),
        provider_revision: instance.provider_revision().clone(),
        software: instance.software().clone(),
        runtime_id: instance.runtime_id(),
        data_line: instance.data_line().clone(),
        data_version: instance.data_version().clone(),
        data_initialized_at: instance.data_initialized_at(),
        endpoints: instance.endpoints().to_vec(),
        desired: instance.desired(),
        startup: instance.startup(),
        lifecycle: instance.lifecycle(),
        last_failure: instance.last_failure().cloned(),
        revision: instance.revision(),
        created_at: instance.created_at(),
        created_by: instance.created_by(),
        retained_at: instance.retained_at(),
        retained_by: instance.retained_by(),
    }
}
