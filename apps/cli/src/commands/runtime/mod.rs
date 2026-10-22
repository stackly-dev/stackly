//! `stackly runtime install`, `list`, and `inspect` (spec 0018, *CLI*,
//! AC-16). They only talk to the daemon: the CLI never resolves a selector,
//! downloads a file, or runs a runtime binary.
//!
//! `install` starts an operation through [`Context::submit`], so it takes
//! `--wait`, `--timeout`, and `--idempotency-key`; `list` and `inspect`
//! read.

mod inspect;
mod install;
mod list;

use clap::Subcommand;
use stackly_protocol::runtime::{GetRuntimeParams, ListRuntimesParams, RuntimeListPage};
use stackly_protocol::{Call, Count, Reply};
use uuid::Uuid;

use self::inspect::InspectArgs;
use self::install::InstallArgs;
use self::list::ListArgs;
use super::execute;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::session::CallSpec;

/// The page size of every `runtime.list` the CLI sends: the most the
/// contract allows.
const PAGE_LIMIT: u64 = 200;

/// The verbs of `stackly runtime`.
#[derive(Debug, Subcommand)]
pub enum RuntimeCommand {
    /// Install a managed runtime: resolve a version, download it, check it, and register it
    #[command(
        after_help = "Examples:\n  stackly runtime install official/postgresql@stable --wait\n  stackly runtime install official/postgresql@18 --dry-run\n  stackly runtime install official/nats@2.15.1 --expect-sha256 ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd --output id"
    )]
    Install(InstallArgs),
    /// List the installed runtimes and the installs that have not ended
    #[command(after_help = "Examples:\n  stackly runtime list\n  stackly runtime list --json")]
    List(ListArgs),
    /// Show one runtime with its artifact, signatures, and compatibility records
    #[command(
        after_help = "Examples:\n  stackly runtime inspect 0192f3a6-0000-7000-8000-0000000000a1\n  stackly runtime inspect 0192f3a6-0000-7000-8000-0000000000a1 --json"
    )]
    Inspect(InspectArgs),
}

impl RuntimeCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Install(args) => execute(args, cx).await,
            Self::List(args) => execute(args, cx).await,
            Self::Inspect(args) => execute(args, cx).await,
        }
    }
}

/// Every runtime the daemon lists, page after page until `next` is null,
/// with the installs of the first page.
///
/// # Errors
///
/// Returns the error of the page that failed.
pub(crate) async fn all_runtimes(cx: &mut Context) -> Result<RuntimeListPage, CliFailure> {
    let mut all = RuntimeListPage {
        items: Vec::new(),
        next: None,
        installing: Vec::new(),
    };
    let mut after: Option<Uuid> = None;
    loop {
        let params = ListRuntimesParams {
            limit: Count::new(PAGE_LIMIT),
            after,
        };
        let page = match cx
            .call(Call::RuntimeList(params), CallSpec::control())
            .await?
        {
            Reply::RuntimeList(page) => *page,
            _ => return Err(CliFailure::unexpected_reply(None)),
        };
        if after.is_none() {
            all.installing = page.installing;
        }
        all.items.extend(page.items);
        match page.next {
            // A cursor that does not move would ask for the same page for
            // ever.
            Some(next) if after != Some(next) => after = Some(next),
            _ => return Ok(all),
        }
    }
}

/// `runtime.get` for `id`.
async fn read_runtime(
    cx: &mut Context,
    id: Uuid,
) -> Result<stackly_protocol::runtime::RuntimeDetail, CliFailure> {
    let call = Call::RuntimeGet(GetRuntimeParams { runtime_id: id });
    match cx.call(call, CallSpec::control()).await? {
        Reply::RuntimeGet(detail) => Ok(*detail),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}
