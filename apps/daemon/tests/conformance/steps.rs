//! The scenario steps of spec 0036 (*Langkah skenario*), from `catalog`
//! to `start_again`, through contract methods on the socket only.

use std::future::Future;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use stackly_core::PreparedLayout;
use stackly_core::provider::ValidatedPackage;
use stackly_core::provider::scenarios::{Expect, Scenario};
use stackly_domain::Capability;
use stackly_protocol::ErrorBody;
use uuid::Uuid;

use super::process::{self, Sample};
use super::site::Daemon;
use super::{
    Failure, ScenarioResult, StepResult, StepStatus, detail, interrupted, skipped_because,
};

/// How often a step polls.
const POLL: Duration = Duration::from_millis(200);
/// The limit of a step that only reads.
const READ_LIMIT: Duration = Duration::from_secs(10);

/// What every scenario shares.
pub struct Ctx {
    /// The connection to the daemon.
    pub daemon: Daemon,
    /// The package under test.
    pub package: ValidatedPackage,
    /// The installed runtime.
    pub runtime_id: Uuid,
    /// The layout of the root, for the `instance.lock` paths.
    pub layout: PreparedLayout,
    /// The step limits from the manifest.
    pub limits: Limits,
    /// The version of the picked artifact.
    pub artifact_version: String,
}

/// The step limits that come from the manifest's `timeouts`, `readiness`,
/// and `health` blocks, with the defaults of spec 0009.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// `timeouts.initSeconds`.
    pub init: Duration,
    /// `timeouts.readinessSeconds`.
    pub readiness: Duration,
    /// `timeouts.stopSeconds`.
    pub stop: Duration,
    /// `timeouts.reloadSeconds`.
    pub reload: Duration,
    /// The health limit: `intervalSeconds` × (`failureThreshold` + 1) +
    /// `attemptTimeoutMilliseconds`, at most 120 s.
    pub health: Duration,
}

impl Limits {
    /// Reads the limits from the package's `provider.yaml`.
    #[must_use]
    pub fn of(package: &ValidatedPackage) -> Self {
        let text = package
            .files()
            .get("provider.yaml")
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default();
        let seconds = |block: &str, key: &str, default: u64| {
            Duration::from_secs(block_value(&text, block, key).unwrap_or(default))
        };
        let interval = block_value(&text, "health", "intervalSeconds").unwrap_or(10);
        let threshold = block_value(&text, "health", "failureThreshold").unwrap_or(3);
        let attempt = block_value(&text, "health", "attemptTimeoutMilliseconds").unwrap_or(2000);
        let health =
            Duration::from_secs(interval * (threshold + 1)) + Duration::from_millis(attempt);
        Self {
            init: seconds("timeouts", "initSeconds", 120),
            readiness: seconds("timeouts", "readinessSeconds", 60),
            stop: seconds("timeouts", "stopSeconds", 30),
            reload: seconds("timeouts", "reloadSeconds", 30),
            health: health.min(Duration::from_secs(120)),
        }
    }
}

/// The whole number `key: <n>` directly under the top level `block:` of a
/// validated manifest written in block style; none when absent.
fn block_value(text: &str, block: &str, key: &str) -> Option<u64> {
    let mut inside = false;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            inside = line.trim_end() == format!("{block}:");
            continue;
        }
        if inside
            && let Some(rest) = line.strip_prefix("  ")
            && !rest.starts_with(' ')
            && let Some(value) = rest
                .strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(':'))
        {
            return value.trim().parse().ok();
        }
    }
    None
}

/// The text of `key: <text>` directly under the top level `block:`.
pub fn block_text(text: &str, block: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            inside = line.trim_end() == format!("{block}:");
            continue;
        }
        if inside
            && let Some(rest) = line.strip_prefix("  ")
            && let Some(value) = rest
                .strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(':'))
        {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// What a step ended in.
pub enum Outcome {
    /// It held, with a detail.
    Pass(Option<Value>),
    /// It did not hold.
    Fail(Failure, Option<Value>),
    /// It does not apply.
    Skip(&'static str),
}

/// One instance the scenario made.
#[derive(Debug, Clone)]
pub struct Inst {
    /// Its ID.
    pub id: Uuid,
    /// The first sample of its main process, from the `process` step.
    pub sample: Option<Sample>,
    /// The process group to check after stop: from the sample, or the PID
    /// of `lastFailure` when the start failed.
    pub pgid: Option<u32>,
    /// The tcp ports of its endpoints.
    pub ports: Vec<u16>,
}

/// The state a scenario carries between steps.
struct Run<'a> {
    ctx: &'a Ctx,
    scenario: &'a Scenario,
    steps: Vec<StepResult>,
    /// The runtime root from `runtime.get`.
    runtime_root: Option<PathBuf>,
    first: Option<Inst>,
    failed: bool,
}

impl Run<'_> {
    /// Runs one step within `limit` and notes it. Returns whether it held
    /// (a skip holds too).
    async fn step<F>(&mut self, name: &'static str, limit: Duration, work: F) -> bool
    where
        F: Future<Output = Outcome>,
    {
        let started = Instant::now();
        let outcome = if interrupted() {
            Outcome::Fail(
                Failure::reason("interrupted", "a signal stopped the run"),
                None,
            )
        } else {
            match tokio::time::timeout(limit, work).await {
                Ok(outcome) => outcome,
                Err(_) => Outcome::Fail(
                    Failure::reason(
                        "step_timeout",
                        format!("the step ran past {} s", limit.as_secs()),
                    ),
                    None,
                ),
            }
        };
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (status, detail, failure) = match outcome {
            Outcome::Pass(detail) => (StepStatus::Pass, detail, None),
            Outcome::Fail(failure, detail) => (StepStatus::Fail, detail, Some(failure)),
            Outcome::Skip(reason) => (StepStatus::Skipped, skipped_because(reason), None),
        };
        let held = status != StepStatus::Fail;
        self.failed |= !held;
        self.steps.push(StepResult {
            name,
            status,
            duration_ms,
            detail,
            failure,
        });
        held
    }

    /// Notes every step in `names` as skipped with `reason`.
    fn skip_rest(&mut self, names: &[&'static str], reason: &'static str) {
        for name in names {
            self.steps.push(StepResult {
                name,
                status: StepStatus::Skipped,
                duration_ms: 0,
                detail: skipped_because(reason),
                failure: None,
            });
        }
    }
}

/// Every step after `create`, in order, for the skips.
const AFTER_CREATE: [&str; 13] = [
    "start",
    "health",
    "connection",
    "process",
    "logs",
    "second_instance",
    "reload",
    "restart",
    "handover",
    "stop",
    "leftovers",
    "start_again",
    "",
];

/// The steps from `index` of [`AFTER_CREATE`] on.
fn rest_from(name: &str) -> Vec<&'static str> {
    AFTER_CREATE
        .iter()
        .copied()
        .skip_while(|step| *step != name)
        .filter(|step| !step.is_empty())
        .collect()
}

/// Runs one scenario (spec 0036, *Langkah skenario*). On the first failed
/// step it stops every instance it made, runs `leftovers`, and returns.
pub async fn run_scenario(ctx: &Ctx, scenario: &Scenario) -> ScenarioResult {
    let mut run = Run {
        ctx,
        scenario,
        steps: Vec::new(),
        runtime_root: None,
        first: None,
        failed: false,
    };
    body(&mut run).await;
    if run.failed {
        stop_everything(ctx).await;
        if let Some(first) = run.first.clone()
            && !run.steps.iter().any(|step| step.name == "leftovers")
        {
            run.step("leftovers", Duration::from_secs(15), leftovers(ctx, &first))
                .await;
        }
    }
    ScenarioResult {
        name: scenario.name.to_string(),
        expect: scenario.expect,
        steps: run.steps,
    }
}

async fn body(run: &mut Run<'_>) {
    let ctx = run.ctx;
    if !run.step("catalog", READ_LIMIT, catalog(ctx)).await {
        return;
    }
    let mut root = None;
    let held = run
        .step("runtime", READ_LIMIT, runtime(ctx, &mut root))
        .await;
    run.runtime_root = root;
    if !held {
        return;
    }
    if !run.step("form", READ_LIMIT, form(ctx)).await {
        return;
    }
    let name = instance_name(run.scenario, 1);
    let expect = run.scenario.expect;
    let mut made = None;
    let create_limit = ctx.limits.init + Duration::from_secs(30);
    let held = run
        .step(
            "create",
            create_limit,
            create(ctx, run.scenario, &name, expect, &mut made),
        )
        .await;
    if !held {
        return;
    }
    let Some(first) = made else {
        // `create_fails` held: nothing was made.
        run.skip_rest(&rest_from("start"), "the scenario expects create to fail");
        return;
    };
    run.first = Some(first.clone());
    let start_limit = ctx.limits.readiness + Duration::from_secs(10);
    if expect == Expect::StartFails {
        let mut pgid = None;
        if !run
            .step("start", start_limit, start_fails(ctx, first.id, &mut pgid))
            .await
        {
            return;
        }
        if let Some(inst) = run.first.as_mut() {
            inst.pgid = pgid;
        }
        let inst = run.first.clone().unwrap_or(first);
        run.step("leftovers", Duration::from_secs(15), leftovers(ctx, &inst))
            .await;
        run.skip_rest(
            &[
                "health",
                "connection",
                "process",
                "logs",
                "second_instance",
                "reload",
                "restart",
                "handover",
                "stop",
                "start_again",
            ],
            "the scenario expects start to fail",
        );
        return;
    }
    if !run.step("start", start_limit, start(ctx, first.id)).await {
        return;
    }
    if let Ok(summary) = get(ctx, first.id).await
        && let Some(inst) = run.first.as_mut()
    {
        inst.ports = tcp_ports(&summary);
    }
    if !run
        .step("health", ctx.limits.health, health(ctx, first.id))
        .await
    {
        return;
    }
    if !run
        .step("connection", READ_LIMIT, connection(ctx, first.id))
        .await
    {
        return;
    }
    let program = run
        .runtime_root
        .as_ref()
        .map(|root| program_path(ctx, root));
    let mut sample = None;
    let held = run
        .step(
            "process",
            Duration::from_secs(30),
            process::check(ctx, first.id, program, &mut sample),
        )
        .await;
    if let Some(inst) = run.first.as_mut() {
        inst.pgid = sample.as_ref().and_then(|sample: &Sample| sample.pgid);
        inst.sample = sample;
    }
    if !held {
        return;
    }
    if !run
        .step("logs", Duration::from_secs(30), logs(ctx, first.id))
        .await
    {
        return;
    }
    let caps = ctx.package.provider_revision().capabilities();
    let multi = caps.iter().any(|cap| cap == Capability::MultiInstance);
    let first_now = run.first.clone().unwrap_or(first);
    if multi {
        let limit = ctx.limits.init
            + ctx.limits.readiness
            + ctx.limits.health
            + ctx.limits.stop
            + Duration::from_secs(60);
        let second = instance_name(run.scenario, 2);
        if !run
            .step(
                "second_instance",
                limit,
                second_instance(ctx, run.scenario, &second, &first_now),
            )
            .await
        {
            return;
        }
    } else {
        run.skip_rest(
            &["second_instance"],
            "the provider has no multiInstance capability",
        );
    }
    let pid = first_now.sample.as_ref().map(|sample| sample.pid);
    if caps.iter().any(|cap| cap == Capability::Reload) {
        let limit = ctx.limits.reload + Duration::from_secs(10);
        if !run
            .step("reload", limit, reload(ctx, first_now.id, pid))
            .await
        {
            return;
        }
    } else {
        run.skip_rest(&["reload"], "the provider has no reload capability");
    }
    let limit = ctx.limits.stop + ctx.limits.readiness + Duration::from_secs(10);
    if !run
        .step("restart", limit, restart(ctx, first_now.id, pid))
        .await
    {
        return;
    }
    if !run
        .step("handover", READ_LIMIT, handover(ctx, first_now.id))
        .await
    {
        return;
    }
    let stop_limit = ctx.limits.stop + Duration::from_secs(10);
    if !run.step("stop", stop_limit, stop(ctx, first_now.id)).await {
        return;
    }
    if !run
        .step(
            "leftovers",
            Duration::from_secs(15),
            leftovers(ctx, &first_now),
        )
        .await
    {
        return;
    }
    let limit = start_limit + ctx.limits.health + stop_limit + Duration::from_secs(15);
    run.step("start_again", limit, start_again(ctx, &first_now))
        .await;
}

/// `c-<scenario>-<n>`.
fn instance_name(scenario: &Scenario, n: u32) -> String {
    format!("c-{}-{n}", scenario.name)
}

/// The runtime program: the runtime root joined with `runtime.executable`.
fn program_path(ctx: &Ctx, root: &std::path::Path) -> PathBuf {
    let text = ctx
        .package
        .files()
        .get("provider.yaml")
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default();
    let executable = block_text(&text, "runtime", "executable").unwrap_or_default();
    let joined = root.join(executable);
    joined.canonicalize().unwrap_or(joined)
}

fn error_failure(error: &ErrorBody) -> Failure {
    Failure::error(error.code(), error.message())
}

/// The `instance.get` summary.
pub async fn get(ctx: &Ctx, id: Uuid) -> Result<Value, Box<ErrorBody>> {
    ctx.daemon
        .call("instance.get", Some(json!({ "instanceId": id })))
        .await
}

/// Polls `operation.get` until the operation ends; returns its detail.
pub async fn wait_operation(ctx: &Ctx, operation: &str) -> Result<Value, Box<ErrorBody>> {
    loop {
        let detail = ctx
            .daemon
            .call("operation.get", Some(json!({ "operationId": operation })))
            .await?;
        let state = detail["operation"]["state"].as_str().unwrap_or_default();
        if matches!(
            state,
            "succeeded" | "failed" | "cancelled" | "needs_recovery"
        ) {
            return Ok(detail);
        }
        if interrupted() {
            return Ok(detail);
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The failure of a finished operation detail.
fn operation_failure(detail: &Value) -> Failure {
    let error = &detail["operation"]["failure"]["error"];
    Failure {
        reason: detail["operation"]["failure"]["reason"]
            .as_str()
            .map(str::to_owned),
        code: error["code"].as_str().map(str::to_owned),
        message: error["message"].as_str().map(str::to_owned),
    }
}

/// Starts an operation with `method` on instance `id` and waits for it.
async fn act(ctx: &Ctx, method: &str, content: Value) -> Result<Value, Failure> {
    let accepted = ctx
        .daemon
        .call_keyed(method, content)
        .await
        .map_err(|error| error_failure(&error))?;
    let Some(operation) = accepted["accepted"]["operation"]["id"].as_str() else {
        // Already in that state: no operation ran.
        return Ok(json!({ "operation": { "state": "succeeded" }, "alreadyInState": true }));
    };
    wait_operation(ctx, operation)
        .await
        .map_err(|error| error_failure(&error))
}

fn state(summary: &Value, field: &str) -> String {
    summary[field]["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

async fn catalog(ctx: &Ctx) -> Outcome {
    let key = ctx.package.key();
    let provider = key.provider().to_string();
    match ctx
        .daemon
        .call(
            "catalog.get_provider",
            Some(json!({ "provider": provider })),
        )
        .await
    {
        Ok(answer) => {
            let revision = answer["summary"]["providerRevision"]
                .as_str()
                .unwrap_or_default();
            let sha = answer["manifestSha256"].as_str().unwrap_or_default();
            if revision == key.revision().as_str() && sha == key.manifest_sha256().to_string() {
                Outcome::Pass(detail(
                    json!({ "provider": provider, "revision": revision }),
                ))
            } else {
                Outcome::Fail(
                    Failure::reason(
                        "catalog_mismatch",
                        format!("the catalog shows revision {revision} and digest {sha}"),
                    ),
                    None,
                )
            }
        }
        Err(error) => Outcome::Fail(error_failure(&error), None),
    }
}

async fn runtime(ctx: &Ctx, root: &mut Option<PathBuf>) -> Outcome {
    let list = match ctx.daemon.call("runtime.list", Some(json!({}))).await {
        Ok(list) => list,
        Err(error) => return Outcome::Fail(error_failure(&error), None),
    };
    let listed = list["items"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["id"] == json!(ctx.runtime_id.to_string()))
    });
    if !listed {
        return Outcome::Fail(
            Failure::reason("runtime_missing", "runtime.list does not show the runtime"),
            None,
        );
    }
    let answer = match ctx
        .daemon
        .call("runtime.get", Some(json!({ "runtimeId": ctx.runtime_id })))
        .await
    {
        Ok(answer) => answer,
        Err(error) => return Outcome::Fail(error_failure(&error), None),
    };
    let summary = &answer["summary"];
    let software = summary["software"].as_str().unwrap_or_default();
    let version = summary["version"].as_str().unwrap_or_default();
    let installed = summary["state"] == "installed";
    if software != ctx.package.software().as_str() || version != ctx.artifact_version || !installed
    {
        return Outcome::Fail(
            Failure::reason(
                "runtime_mismatch",
                format!(
                    "runtime.get shows {software} {version} {}",
                    summary["state"]
                ),
            ),
            None,
        );
    }
    *root = answer["path"].as_str().map(PathBuf::from);
    Outcome::Pass(detail(json!({ "software": software, "version": version })))
}

async fn form(ctx: &Ctx) -> Outcome {
    match create_form(ctx).await {
        Ok(form) => {
            let fields = form["fields"].as_array().map_or(0, Vec::len);
            let expected = ctx.package.settings_schema().fields().count();
            let unknown: Vec<&str> = form["fields"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|field| {
                    !matches!(
                        field["kind"].as_str(),
                        Some("text" | "integer" | "boolean" | "choice" | "list")
                    )
                })
                .filter_map(|field| field["key"].as_str())
                .collect();
            if fields != expected {
                Outcome::Fail(
                    Failure::reason(
                        "form_mismatch",
                        format!("the form has {fields} fields, the schema {expected}"),
                    ),
                    None,
                )
            } else if !unknown.is_empty() {
                Outcome::Fail(
                    Failure::reason(
                        "form_unrecognized",
                        format!("fields of an unknown kind: {}", unknown.join(", ")),
                    ),
                    None,
                )
            } else {
                Outcome::Pass(detail(json!({ "fields": fields })))
            }
        }
        Err(error) => Outcome::Fail(error_failure(&error), None),
    }
}

async fn create_form(ctx: &Ctx) -> Result<Value, Box<ErrorBody>> {
    let key = ctx.package.key();
    ctx.daemon
        .call(
            "instance.get_create_form",
            Some(json!({
                "runtimeId": ctx.runtime_id,
                "provider": key.provider().to_string(),
                "revision": key.revision().to_string(),
            })),
        )
        .await
}

/// The settings, endpoints, and secrets of a create: the form defaults
/// under the scenario's settings, every tcp and udp endpoint on an
/// automatic port, every secret generated.
fn draft(form: &Value, scenario: &Scenario) -> (Value, Vec<Value>, Vec<String>) {
    let mut settings = Map::new();
    let mut secrets = Vec::new();
    for field in form["fields"].as_array().into_iter().flatten() {
        let Some(key) = field["key"].as_str() else {
            continue;
        };
        if field["secret"] == true {
            secrets.push(key.to_owned());
            continue;
        }
        if field["managed"] == true {
            continue;
        }
        if let Some(value) = field["default"].get("content") {
            settings.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(own) = scenario.settings.as_object() {
        for (key, value) in own {
            settings.insert(key.clone(), value.clone());
        }
    }
    let endpoints = form["endpoints"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|endpoint| matches!(endpoint["transport"].as_str(), Some("tcp" | "udp")))
        .filter_map(|endpoint| endpoint["name"].as_str())
        .map(|name| json!({ "name": name, "port": { "type": "auto" } }))
        .collect();
    (Value::Object(settings), endpoints, secrets)
}

/// Creates instance `name`; `made` gets it when the operation succeeded.
async fn create(
    ctx: &Ctx,
    scenario: &Scenario,
    name: &str,
    expect: Expect,
    made: &mut Option<Inst>,
) -> Outcome {
    let form = match create_form(ctx).await {
        Ok(form) => form,
        Err(error) => return Outcome::Fail(error_failure(&error), None),
    };
    let (settings, endpoints, secrets) = draft(&form, scenario);
    let key = ctx.package.key();
    let provider =
        json!({ "name": key.provider().to_string(), "revision": key.revision().to_string() });
    let preflight = ctx
        .daemon
        .call(
            "instance.preflight",
            Some(json!({
                "name": name,
                "displayName": null,
                "provider": provider,
                "runtimeId": ctx.runtime_id,
                "endpoints": endpoints,
                "settings": settings,
                "secrets": secrets.iter().map(|field| json!({ "field": field, "source": { "type": "generate" } })).collect::<Vec<_>>(),
                "start": false,
                "startup": null,
            })),
        )
        .await;
    let refused = match &preflight {
        Err(error) => Some(error_failure(error)),
        Ok(answer) if answer["ready"] != true => {
            let codes: Vec<String> = answer["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|check| check["status"] != "ok")
                .filter_map(|check| check["error"]["code"].as_str().map(str::to_owned))
                .collect();
            Some(Failure {
                reason: Some("preflight_not_ready".to_owned()),
                code: codes.first().cloned(),
                message: Some(format!("preflight checks not ok: {}", codes.join(", "))),
            })
        }
        Ok(_) => None,
    };
    if let Some(failure) = refused {
        return if expect == Expect::CreateFails {
            Outcome::Pass(detail(
                json!({ "refusedBy": "instance.preflight", "code": failure.code }),
            ))
        } else {
            Outcome::Fail(failure, None)
        };
    }
    let params = json!({
        "name": name,
        "displayName": null,
        "provider": provider,
        "runtimeId": ctx.runtime_id,
        "endpoints": endpoints,
        "settings": settings,
        "secrets": secrets.iter().map(|field| json!({ "field": field, "input": { "type": "generate" } })).collect::<Vec<_>>(),
        "startup": null,
        "duplicateOf": null,
    });
    let accepted = match ctx.daemon.call_keyed("instance.create", params).await {
        Ok(accepted) => accepted,
        Err(error) => {
            return if expect == Expect::CreateFails {
                Outcome::Pass(detail(
                    json!({ "refusedBy": "instance.create", "code": error.code().to_string() }),
                ))
            } else {
                Outcome::Fail(error_failure(&error), None)
            };
        }
    };
    let operation = &accepted["accepted"]["operation"];
    let (Some(op), Some(id)) = (
        operation["id"].as_str(),
        operation["resource"]["id"]
            .as_str()
            .and_then(|id| id.parse::<Uuid>().ok()),
    ) else {
        return Outcome::Fail(
            Failure::reason("no_operation", "instance.create started no operation"),
            None,
        );
    };
    let finished = match wait_operation(ctx, op).await {
        Ok(detail) => detail,
        Err(error) => return Outcome::Fail(error_failure(&error), None),
    };
    let succeeded = finished["operation"]["state"] == "succeeded";
    if expect == Expect::CreateFails {
        return if succeeded {
            *made = Some(Inst {
                id,
                sample: None,
                pgid: None,
                ports: Vec::new(),
            });
            Outcome::Fail(
                Failure::reason(
                    "unexpected_success",
                    "the create succeeded though the scenario expects it to fail",
                ),
                None,
            )
        } else {
            let failure = operation_failure(&finished);
            Outcome::Pass(detail(
                json!({ "refusedBy": "operation", "code": failure.code }),
            ))
        };
    }
    if !succeeded {
        return Outcome::Fail(operation_failure(&finished), None);
    }
    let summary = match get(ctx, id).await {
        Ok(summary) => summary,
        Err(error) => return Outcome::Fail(error_failure(&error), None),
    };
    *made = Some(Inst {
        id,
        sample: None,
        pgid: None,
        ports: Vec::new(),
    });
    if summary["lifecycle"] != "stopped" || summary["initialized"] != true {
        return Outcome::Fail(
            Failure::reason(
                "create_state",
                format!(
                    "after create the lifecycle is {} and initialized {}",
                    summary["lifecycle"], summary["initialized"]
                ),
            ),
            None,
        );
    }
    Outcome::Pass(detail(json!({ "instance": name })))
}

/// The tcp ports of an instance summary.
fn tcp_ports(summary: &Value) -> Vec<u16> {
    summary["endpoints"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|endpoint| endpoint["transport"] == "tcp")
        .filter_map(|endpoint| endpoint["port"].as_u64())
        .filter_map(|port| u16::try_from(port).ok())
        .collect()
}

async fn start(ctx: &Ctx, id: Uuid) -> Outcome {
    let finished = match act(ctx, "instance.start", json!({ "instanceId": id })).await {
        Ok(finished) => finished,
        Err(failure) => return Outcome::Fail(failure, None),
    };
    if finished["operation"]["state"] != "succeeded" {
        return Outcome::Fail(operation_failure(&finished), None);
    }
    match get(ctx, id).await {
        Ok(summary)
            if summary["lifecycle"] == "running" && state(&summary, "readiness") == "ready" =>
        {
            Outcome::Pass(None)
        }
        Ok(summary) => Outcome::Fail(
            Failure::reason(
                "start_state",
                format!(
                    "after start the lifecycle is {} and readiness {}",
                    summary["lifecycle"],
                    state(&summary, "readiness")
                ),
            ),
            None,
        ),
        Err(error) => Outcome::Fail(error_failure(&error), None),
    }
}

/// `start` of a scenario that expects it to fail; notes the PID of
/// `lastFailure` for `leftovers`.
async fn start_fails(ctx: &Ctx, id: Uuid, pgid: &mut Option<u32>) -> Outcome {
    let finished = match act(ctx, "instance.start", json!({ "instanceId": id })).await {
        Ok(finished) => finished,
        Err(failure) => {
            return Outcome::Pass(detail(
                json!({ "refusedBy": "instance.start", "code": failure.code }),
            ));
        }
    };
    if finished["operation"]["state"] == "succeeded" {
        return Outcome::Fail(
            Failure::reason(
                "unexpected_success",
                "the start succeeded though the scenario expects it to fail",
            ),
            None,
        );
    }
    let failure = operation_failure(&finished);
    match get(ctx, id).await {
        Ok(summary) if !summary["lastFailure"].is_null() => {
            *pgid = summary["lastFailure"]["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok());
            Outcome::Pass(detail(json!({
                "code": failure.code,
                "lastFailure": summary["lastFailure"]["code"],
            })))
        }
        Ok(_) => Outcome::Fail(
            Failure::reason(
                "no_last_failure",
                "the start failed but lastFailure is empty",
            ),
            None,
        ),
        Err(error) => Outcome::Fail(error_failure(&error), None),
    }
}

async fn health(ctx: &Ctx, id: Uuid) -> Outcome {
    loop {
        match get(ctx, id).await {
            Ok(summary) if state(&summary, "health") == "healthy" => return Outcome::Pass(None),
            Ok(_) => {}
            Err(error) => return Outcome::Fail(error_failure(&error), None),
        }
        if interrupted() {
            return Outcome::Fail(
                Failure::reason("interrupted", "a signal stopped the run"),
                None,
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn connection(ctx: &Ctx, id: Uuid) -> Outcome {
    let expected = ctx.package.manifest().connections().len();
    if expected == 0 {
        return Outcome::Skip("the manifest has no connection block");
    }
    let detail_value = match ctx
        .daemon
        .call("instance.get_detail", Some(json!({ "instanceId": id })))
        .await
    {
        Ok(detail) => detail,
        Err(error) => return Outcome::Fail(error_failure(&error), None),
    };
    let entries = detail_value["connections"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let bad: Vec<String> = entries
        .iter()
        .filter(|entry| {
            let masked = entry["masked"].as_str().unwrap_or_default();
            entry["state"] != "available" || !absolute_uri(masked)
        })
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect();
    if entries.len() != expected {
        return Outcome::Fail(
            Failure::reason(
                "connection_mismatch",
                format!(
                    "{} connections, the manifest declares {expected}",
                    entries.len()
                ),
            ),
            None,
        );
    }
    if !bad.is_empty() {
        return Outcome::Fail(
            Failure::reason(
                "connection_unavailable",
                format!(
                    "entries not available as an absolute URI: {}",
                    bad.join(", ")
                ),
            ),
            None,
        );
    }
    Outcome::Pass(detail(json!({ "connections": entries.len() })))
}

/// `scheme://...` with a scheme of letters, digits, `+`, `-`, and `.`.
fn absolute_uri(text: &str) -> bool {
    text.split_once("://").is_some_and(|(scheme, rest)| {
        !rest.is_empty()
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

async fn logs(ctx: &Ctx, id: Uuid) -> Outcome {
    // A page of 1000 lines, or the same count twice in a row: the store
    // holds no more yet.
    let mut last = None;
    loop {
        let page = match ctx
            .daemon
            .call(
                "instance.logs_read",
                Some(json!({ "instance": id, "limit": 1000 })),
            )
            .await
        {
            Ok(page) => page,
            Err(error) => return Outcome::Fail(error_failure(&error), None),
        };
        let lines = page["entries"].as_array().map_or(0, Vec::len);
        if lines >= 1000 || last == Some(lines) {
            return Outcome::Pass(detail(json!({ "lines": lines })));
        }
        last = Some(lines);
        if interrupted() {
            return Outcome::Fail(
                Failure::reason("interrupted", "a signal stopped the run"),
                None,
            );
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn second_instance(ctx: &Ctx, scenario: &Scenario, name: &str, first: &Inst) -> Outcome {
    let mut made = None;
    if let Outcome::Fail(failure, detail) =
        create(ctx, scenario, name, Expect::Ready, &mut made).await
    {
        return Outcome::Fail(failure, detail);
    }
    let Some(second) = made else {
        return Outcome::Fail(
            Failure::reason("no_instance", "the second create made nothing"),
            None,
        );
    };
    for outcome in [start(ctx, second.id).await, health(ctx, second.id).await] {
        if let Outcome::Fail(failure, detail) = outcome {
            return Outcome::Fail(failure, detail);
        }
    }
    let (one, two) = match (get(ctx, first.id).await, get(ctx, second.id).await) {
        (Ok(one), Ok(two)) => (one, two),
        (Err(error), _) | (_, Err(error)) => return Outcome::Fail(error_failure(&error), None),
    };
    let pid = |summary: &Value| summary["process"]["pid"].as_u64();
    let differs = pid(&one) != pid(&two) && {
        let a = tcp_ports(&one);
        tcp_ports(&two).iter().all(|port| !a.contains(port))
    };
    let still_ready = state(&one, "readiness") == "ready";
    if let Outcome::Fail(failure, detail) = stop(ctx, second.id).await {
        return Outcome::Fail(failure, detail);
    }
    if !differs {
        return Outcome::Fail(
            Failure::reason("not_isolated", "the two instances share a PID or a port"),
            None,
        );
    }
    if !still_ready {
        return Outcome::Fail(
            Failure::reason("first_not_ready", "the first instance is no longer ready"),
            None,
        );
    }
    Outcome::Pass(detail(json!({ "instance": name })))
}

async fn reload(ctx: &Ctx, id: Uuid, pid: Option<u32>) -> Outcome {
    let finished = match act(ctx, "instance.reload", json!({ "instanceId": id })).await {
        Ok(finished) => finished,
        Err(failure) => return Outcome::Fail(failure, None),
    };
    if finished["operation"]["state"] != "succeeded" {
        return Outcome::Fail(operation_failure(&finished), None);
    }
    match ready_pid(ctx, id).await {
        Ok(now) if pid.is_none() || now == pid => Outcome::Pass(None),
        Ok(now) => Outcome::Fail(
            Failure::reason(
                "pid_changed",
                format!("the PID changed from {pid:?} to {now:?} on reload"),
            ),
            None,
        ),
        Err(outcome) => outcome,
    }
}

async fn restart(ctx: &Ctx, id: Uuid, pid: Option<u32>) -> Outcome {
    let finished = match act(ctx, "instance.restart", json!({ "instanceId": id })).await {
        Ok(finished) => finished,
        Err(failure) => return Outcome::Fail(failure, None),
    };
    if finished["operation"]["state"] != "succeeded" {
        return Outcome::Fail(operation_failure(&finished), None);
    }
    match ready_pid(ctx, id).await {
        Ok(now) if now.is_some() && now != pid => Outcome::Pass(None),
        Ok(now) => Outcome::Fail(
            Failure::reason("pid_kept", format!("the PID stayed {now:?} across restart")),
            None,
        ),
        Err(outcome) => outcome,
    }
}

/// Waits until the instance is ready again; returns its PID.
async fn ready_pid(ctx: &Ctx, id: Uuid) -> Result<Option<u32>, Outcome> {
    loop {
        let summary = get(ctx, id)
            .await
            .map_err(|error| Outcome::Fail(error_failure(&error), None))?;
        if state(&summary, "readiness") == "ready" && summary["lifecycle"] == "running" {
            return Ok(summary["process"]["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok()));
        }
        if interrupted() {
            return Err(Outcome::Fail(
                Failure::reason("interrupted", "a signal stopped the run"),
                None,
            ));
        }
        tokio::time::sleep(POLL).await;
    }
}

async fn handover(ctx: &Ctx, id: Uuid) -> Outcome {
    match ctx.daemon.call("daemon.restart_preview", None).await {
        Ok(preview) => {
            let reason = preview["instances"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|instance| instance["instanceId"] == json!(id.to_string()))
                .and_then(|instance| instance["reason"].as_str())
                .unwrap_or_default()
                .to_owned();
            let handover = if reason == "handover_proven" {
                "proven"
            } else {
                "unproven"
            };
            Outcome::Pass(detail(json!({ "handover": handover, "reason": reason })))
        }
        Err(error) => Outcome::Fail(error_failure(&error), None),
    }
}

async fn stop(ctx: &Ctx, id: Uuid) -> Outcome {
    let finished = match act(
        ctx,
        "instance.stop",
        json!({ "instanceId": id, "overrideLeases": false }),
    )
    .await
    {
        Ok(finished) => finished,
        Err(failure) => return Outcome::Fail(failure, None),
    };
    if finished["operation"]["state"] != "succeeded" {
        return Outcome::Fail(operation_failure(&finished), None);
    }
    match get(ctx, id).await {
        Ok(summary) if summary["lifecycle"] == "stopped" => Outcome::Pass(None),
        Ok(summary) => Outcome::Fail(
            Failure::reason(
                "stop_state",
                format!("after stop the lifecycle is {}", summary["lifecycle"]),
            ),
            None,
        ),
        Err(error) => Outcome::Fail(error_failure(&error), None),
    }
}

/// `leftovers` for `inst`, with the ports it had while running.
async fn leftovers(ctx: &Ctx, inst: &Inst) -> Outcome {
    let lock = ctx
        .layout
        .layout()
        .instance_paths(
            inst.id
                .to_string()
                .parse()
                .expect("the daemon hands out instance IDs"),
        )
        .lock;
    process::leftovers(&lock, inst.pgid, &inst.ports)
}

async fn start_again(ctx: &Ctx, first: &Inst) -> Outcome {
    for outcome in [start(ctx, first.id).await, health(ctx, first.id).await] {
        if let Outcome::Fail(failure, detail) = outcome {
            return Outcome::Fail(failure, detail);
        }
    }
    let mut inst = first.clone();
    if let Ok(summary) = get(ctx, first.id).await {
        inst.ports = tcp_ports(&summary);
        let pid = summary["process"]["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok());
        inst.pgid = pid.and_then(process::pgid).or(inst.pgid);
    }
    if let Outcome::Fail(failure, detail) = stop(ctx, first.id).await {
        return Outcome::Fail(failure, detail);
    }
    leftovers(ctx, &inst).await
}

/// End of a failed scenario and end of run step 1: stops every instance
/// through `instance.preview_stop_all` and `instance.stop_all` with the
/// previewed IDs, then `instance.force_stop` for any that did not stop.
pub async fn stop_everything(ctx: &Ctx) {
    let Ok(preview) = ctx.daemon.call("instance.preview_stop_all", None).await else {
        return;
    };
    let ids: Vec<Value> = preview["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("instanceId").cloned())
        .collect();
    if ids.is_empty() {
        return;
    }
    if let Ok(result) = ctx
        .daemon
        .call_keyed("instance.stop_all", json!({ "expectedInstanceIds": ids }))
        .await
        && let Some(operation) = result["accepted"]["operation"]["id"].as_str()
    {
        let _ = tokio::time::timeout(
            ctx.limits.stop + Duration::from_secs(10),
            wait_operation(ctx, operation),
        )
        .await;
    }
    for id in ids
        .iter()
        .filter_map(|id| id.as_str()?.parse::<Uuid>().ok())
    {
        let running = get(ctx, id).await.is_ok_and(|summary| {
            !matches!(summary["lifecycle"].as_str(), Some("stopped" | "failed"))
        });
        if running {
            let _ = tokio::time::timeout(
                ctx.limits.stop + Duration::from_secs(10),
                act(ctx, "instance.force_stop", json!({ "instanceId": id })),
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_value_is_read_only_directly_under_its_block() {
        let text = "timeouts:\n  initSeconds: 30\n  stopSeconds: 10\nreadiness:\n  probe: { type: http }\n  intervalMilliseconds: 100\nhealth:\n  probe:\n    intervalSeconds: 99\n  intervalSeconds: 5\n";

        assert_eq!(block_value(text, "timeouts", "initSeconds"), Some(30));
        assert_eq!(block_value(text, "timeouts", "reloadSeconds"), None);
        assert_eq!(block_value(text, "health", "intervalSeconds"), Some(5));
        assert_eq!(block_value(text, "readiness", "stopSeconds"), None);
    }

    #[test]
    fn an_absolute_uri_has_a_scheme_and_a_rest() {
        assert!(absolute_uri("http://127.0.0.1:1/config"));
        assert!(absolute_uri("postgresql://u:****@/db?host=/x"));
        assert!(!absolute_uri("/tmp/socket"));
        assert!(!absolute_uri("1http://x"));
        assert!(!absolute_uri("http://"));
    }
}
