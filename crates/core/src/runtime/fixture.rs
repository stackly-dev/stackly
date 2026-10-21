//! Installing a cached artifact directly, for tests (spec 0018, *API
//! surface*): the stage bodies of the `install_runtime` operation run one
//! after the other with no operation engine and no network, against an
//! artifact file that is already on disk.
//!
//! This module exists only with the `fixture` feature, which only a dev
//! dependency turns on.

use std::path::Path;
use std::sync::Arc;

use stackly_domain::{ArtifactRecord, Registry, Runtime, RuntimeId, WriteBatch};
use stackly_platform::{Platform, StopFlag};

use super::download::{CachePaths, DownloadPolicy};
use super::install::{
    Env, FinalFacts, InstallError, InstallInput, Plan, ScratchPaths, commit_stage, download_stage,
    extract_stage, final_batch, validate_stage, verify_stage,
};
use super::validate::VersionCheckSpec;
use crate::layout::PreparedLayout;
use crate::ownership::remove_owned_tree;
use crate::provider::ValidatedPackage;

/// Copies `cache_file` into the download cache as `<sha256>`, unless an
/// entry is already there.
fn fill_cache(env: &Env, plan: &Plan, cache_file: &Path) -> Result<(), InstallError> {
    let fs = env.platform.fs();
    let paths = CachePaths::new(&env.layout.layout().downloads_dir(), &plan.sha256);
    let io = |op: &'static str| InstallError::Io { op, root: "cache" };
    if fs
        .symlink_metadata(&paths.entry)
        .map_err(|_| io("inspect the cache entry"))?
        .is_some()
    {
        return Ok(());
    }
    let mut source = fs
        .open_read(cache_file)
        .map_err(|_| io("open the cache file"))?;
    let mut part = fs
        .open_append(&paths.part)
        .map_err(|_| io("open the partial file"))?;
    std::io::copy(&mut source, &mut part).map_err(|_| io("copy the cache file"))?;
    part.sync().map_err(|_| io("sync the partial file"))?;
    drop(part);
    fs.rename_no_replace(&paths.part, &paths.entry)
        .map_err(|_| io("rename the partial file"))
}

/// Installs the artifact `artifact_id` of `package` from `cache_file`, as
/// the `install_runtime` operation would, and returns the runtime and the
/// artifact record it wrote.
///
/// It copies `cache_file` to `downloads_dir()/<sha256>`, runs the `download`
/// (a cache hit), `verify_digest`, `extract`, `validate_binary` (with the
/// exec check when the package declares `runtime.versionCheck`), and
/// `commit` stages and the final batch in a temporary scratch folder under
/// `operations_dir()`, and writes `InsertArtifact` and `InsertRuntime` to
/// `registry` itself, so the records it returns are already committed and a
/// test must not commit them again.
///
/// # Errors
///
/// Returns the [`InstallError`] of the first stage that fails, the same
/// errors the operation would fail with. An artifact the package does not
/// have, or a file whose digest is not the catalog's, fails that way too.
pub fn install_cached_artifact(
    platform: &Platform,
    layout: &PreparedLayout,
    registry: &dyn Registry,
    package: &ValidatedPackage,
    artifact_id: &str,
    cache_file: &Path,
) -> Result<(Runtime, ArtifactRecord), InstallError> {
    let artifact = package
        .versions()
        .artifacts()
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or_else(|| {
            InstallError::Internal(format!("the package has no artifact {artifact_id}"))
        })?;
    let host = platform.info().host_facts();
    let check = package.version_check().map(VersionCheckSpec::from_check);
    let input = InstallInput::new(
        package.software(),
        &package.key().provider().to_string(),
        "fixture",
        &host,
        artifact,
        &[],
        package.key(),
        check.as_ref(),
    )
    .map_err(|error| InstallError::Internal(error.to_string()))?;
    let plan =
        Plan::from_input(&input).map_err(|error| InstallError::Internal(error.to_string()))?;
    let env = Env {
        platform: Arc::new(platform.clone()),
        layout: Arc::new(layout.clone()),
        policy: DownloadPolicy::default(),
    };
    fill_cache(&env, &plan, cache_file)?;

    let runtime_id = RuntimeId::generate();
    let operation_id = format!("fixture-{runtime_id}");
    let scratch = ScratchPaths::new(&layout.layout().operations_dir().join(&operation_id));
    env.platform
        .fs()
        .create_dir_private(&scratch.scratch)
        .map_err(|_| InstallError::Io {
            op: "create the scratch folder",
            root: "data",
        })?;
    let result = run_stages(&env, &plan, &scratch, &operation_id, registry, runtime_id);
    if let Err(error) = remove_owned_tree(&env.platform, &env.layout, &scratch.scratch) {
        tracing::warn!(%error, "the scratch folder of a fixture install was not removed");
    }
    result?;

    let runtime = registry
        .runtime(runtime_id)
        .ok()
        .and_then(|read| read.value)
        .ok_or_else(|| InstallError::Internal("the runtime was not written".to_owned()))?;
    let record = registry
        .artifact(&plan.sha256)
        .ok()
        .and_then(|read| read.value)
        .ok_or_else(|| InstallError::Internal("the artifact was not written".to_owned()))?;
    Ok((runtime, record))
}

fn run_stages(
    env: &Env,
    plan: &Plan,
    scratch: &ScratchPaths,
    operation_id: &str,
    registry: &dyn Registry,
    runtime_id: RuntimeId,
) -> Result<(), InstallError> {
    let stop = StopFlag::new();
    let none = |_: u64, _: u64| {};
    let downloaded = download_stage(env, plan, &stop, &none)?;
    verify_stage(env, plan, &stop, &none)?;
    let (report, extracted) = extract_stage(env, plan, scratch, operation_id, &stop, &none)?;
    let validated = validate_blocking(env, plan, scratch, report.entries, &stop)?;
    commit_stage(env, plan, scratch, operation_id, registry)?;
    let facts = FinalFacts {
        runtime_id,
        validate: validated,
        extract: extracted,
        source: downloaded.source,
    };
    let batch = final_batch(env, plan, &facts, registry)?;
    let mut write = WriteBatch::new(batch.ops);
    write.expected_generation = batch.expected_generation;
    registry.commit(write).map_err(|error| {
        tracing::warn!(%error, "the final batch of a fixture install was refused");
        InstallError::Io {
            op: "write the registry",
            root: "data",
        }
    })?;
    Ok(())
}

/// Runs the async validation on a thread of its own with a runtime of its
/// own, so the caller may or may not already be inside one.
fn validate_blocking(
    env: &Env,
    plan: &Plan,
    scratch: &ScratchPaths,
    entries: Vec<super::archive::EntryRecord>,
    stop: &StopFlag,
) -> Result<super::install::ValidateCheckpoint, InstallError> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .map_err(|error| InstallError::Internal(error.to_string()))?;
                runtime.block_on(validate_stage(
                    env,
                    plan,
                    scratch,
                    Some(entries),
                    stop,
                    &|_, _| {},
                ))
            })
            .join()
            .unwrap_or_else(|_| Err(InstallError::Internal("validation panicked".to_owned())))
    })
}
