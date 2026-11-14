//! The data side of `env.down` with real runtimes (spec 0047 AC-7, AC-26,
//! BR-006), next to `env_down_native`: after Up of the ERP example manifest
//! every provider that keeps data gets a sentinel written through its own
//! protocol (PostgreSQL and MySQL through the client of the installed
//! runtime, ClickHouse over HTTP, Valkey over RESP, NATS into a JetStream file
//! stream), then Down, then Up again, and each sentinel is read back. The data
//! folder of each instance is compared by SHA-256 of every file before Down
//! and after it, outside the files only a running process owns (named per
//! provider); a file a clean shutdown rewrites is recorded in the evidence
//! note, never left out quietly, and no data file may go away or lose its
//! content. The runtime folder is compared by name and size before Down,
//! after it, and after the second Up. The runtimes come from the runtime
//! cache of `cargo xtask` with no download, so a missing artifact ends the
//! test `BLOCKED`; secrets live in the test root's fake store. Native:
//! `cargo test -p stacklyd --test env_down_data_native -- --ignored`.
#![cfg(target_os = "macos")]

mod common;
#[path = "supervisor/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use sha2::Digest as _;
use stackly_core::catalog::{
    Catalog, CatalogEntry, EntryOrigin, ProviderEntry, TileColor, TileHint,
};
use stackly_core::provider::{ArtifactStatus, NameCheck, ValidatedPackage, read_package};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_ipc::CallOptions;
use stackly_protocol::env::{
    EnvDownParams, EnvDownPreview, EnvDownReply, EnvGetParams, EnvPlan, EnvPlanParams,
    EnvPreviewDownParams, EnvSource, EnvUpParams, EnvUpReply, EnvironmentDetail,
};
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_protocol::secret::{SecretInput, SecretMode, SecretText, SetSecretParams};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, Source, evidence};
use stackly_testkit::{RuntimeCache, allocate_port};
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Api, Site, fast, key};

/// Who can fill the runtime cache (spec 0002).
const OWNER: &str = "user";
/// The six official packages of the ERP manifest, by folder.
const PROVIDERS: [&str; 6] = [
    "postgresql",
    "mysql",
    "nats",
    "clickhouse",
    "valkey",
    "traefik",
];
/// The ports the ERP manifest names; each one is replaced by a free port.
const PORTS: [u16; 9] = [5432, 3306, 4222, 8222, 8123, 9000, 6379, 8000, 8082];
/// How long six real runtimes may take to create, start, stop, and be ready.
const UP_PATIENCE: Duration = Duration::from_secs(600);
/// How long one sentinel request may take.
const IO_TIMEOUT: Duration = Duration::from_secs(20);

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn package(name: &str) -> ValidatedPackage {
    let dir = repo().join("providers").join(name);
    let files =
        read_package(&stackly_platform_macos::MacosFilesystem, &dir).expect("the package reads");
    ValidatedPackage::from_files(files, NameCheck::FolderName).expect("the package validates")
}

fn catalog() -> Catalog {
    let entries: Vec<_> = PROVIDERS
        .iter()
        .map(|name| {
            CatalogEntry::Provider(ProviderEntry {
                package: Arc::new(package(name)),
                tile: TileHint {
                    mark: "Te".to_owned(),
                    tile: TileColor::Blue,
                },
                notes: Vec::new(),
                origin: EntryOrigin::Catalog,
            })
        })
        .collect();
    Catalog::for_tests(&entries)
}

async fn start(site: &Site) -> DaemonHandle {
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(catalog()));
    site.start_with(config).await
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

fn manifest(site: &Site) -> PathBuf {
    let mut text = std::fs::read_to_string(
        repo().join("crates/core/tests/fixtures/environments/erp-development.yaml"),
    )
    .expect("the ERP manifest reads");
    for port in PORTS {
        let free = allocate_port().expect("a free port");
        text = text.replace(&format!("port: {port}}}"), &format!("port: {free}}}"));
    }
    let dir = site.root.path().join("erp");
    std::fs::create_dir_all(&dir).expect("the project folder can be made");
    let path = dir.join("stackly.yaml");
    std::fs::write(&path, text).expect("the manifest writes");
    path
}

async fn plan(api: &Api, path: &Path) -> EnvPlan {
    match api
        .client
        .call(Call::EnvPlan(EnvPlanParams {
            source: EnvSource::File(path.display().to_string()),
            locked: false,
        }))
        .await
        .expect("the plan answers")
    {
        Reply::EnvPlan(plan) => *plan,
        other => panic!("not a plan: {other:?}"),
    }
}

async fn up(api: &Api, path: &Path) -> EnvUpReply {
    let planned = plan(api, path).await;
    let digest = planned
        .plan_digest
        .clone()
        .unwrap_or_else(|| panic!("the plan is applicable: {:?}", planned.blockers));
    match api
        .client
        .call_detailed(
            Call::EnvUp(EnvUpParams {
                source: EnvSource::File(path.display().to_string()),
                expected_plan_digest: digest,
                locked: false,
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("the up starts")
    {
        Reply::EnvUp(reply) => *reply,
        other => panic!("not an up reply: {other:?}"),
    }
}

async fn wait_final(api: &Api, id: Uuid) -> OperationDetail {
    let deadline = tokio::time::Instant::now() + UP_PATIENCE;
    loop {
        let detail = api.operation(id).await;
        if matches!(
            detail.operation.state,
            OperationState::Succeeded
                | OperationState::Failed
                | OperationState::Cancelled
                | OperationState::NeedsRecovery
        ) {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the operation did not end: {detail:?}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn get(api: &Api, id: Uuid) -> EnvironmentDetail {
    match api
        .client
        .call(Call::EnvGet(EnvGetParams {
            id,
            check_manifest: false,
        }))
        .await
        .expect("the detail answers")
    {
        Reply::EnvGet(detail) => *detail,
        other => panic!("not a detail: {other:?}"),
    }
}

/// The fixture password of the service `alias`.
fn password(alias: &str) -> String {
    format!("stackly-test-erp-{alias}")
}

async fn set_secrets(api: &Api, planned: &EnvPlan) {
    for item in &planned.items {
        for secret in &item.secrets {
            let Some(reference) = &secret.reference else {
                continue;
            };
            let reply = api
                .client
                .call(Call::SecretSet(SetSecretParams {
                    reference: reference.clone(),
                    input: SecretInput::Value(SecretText::new(password(&item.alias))),
                    mode: SecretMode::Upsert,
                    expected_revision: None,
                    confirm_stored_only: false,
                }))
                .await;
            assert!(reply.is_ok(), "the secret {reference} is stored: {reply:?}");
        }
    }
}

async fn down(api: &Api, id: Uuid) -> (EnvDownPreview, EnvDownReply) {
    let shown = match api
        .client
        .call(Call::EnvPreviewDown(EnvPreviewDownParams { id }))
        .await
        .expect("the preview answers")
    {
        Reply::EnvPreviewDown(preview) => *preview,
        other => panic!("not a preview: {other:?}"),
    };
    let reply = match api
        .client
        .call_detailed(
            Call::EnvDown(EnvDownParams {
                id,
                expected_stop_instance_ids: shown
                    .stop
                    .iter()
                    .map(|item| item.instance_id)
                    .collect(),
            }),
            CallOptions::default().with_idempotency_key(key()),
        )
        .await
        .expect("the down starts")
    {
        Reply::EnvDown(reply) => *reply,
        other => panic!("not a down reply: {other:?}"),
    };
    (shown, reply)
}

fn install_from_cache(
    site: &Site,
    planned: &EnvPlan,
    ev: &stackly_testkit::evidence::Evidence,
) -> Result<(), Vec<String>> {
    let cache = RuntimeCache::open();
    let mut missing = Vec::new();
    let mut installs = Vec::new();
    for item in &planned.items {
        let runtime = item.runtime.as_ref().expect("a create names its runtime");
        let name = item
            .provider
            .strip_prefix("official/")
            .expect("an official provider");
        let package = package(name);
        let size = package
            .versions()
            .artifacts()
            .iter()
            .find(|artifact| artifact.id == runtime.artifact_id)
            .and_then(|artifact| match &artifact.status {
                ArtifactStatus::Available { size, .. } => Some(*size),
                _ => None,
            })
            .unwrap_or_else(|| panic!("versions.json has {} available", runtime.artifact_id));
        match cache
            .as_ref()
            .and_then(|cache| cache.entry(&runtime.artifact_sha256, size))
        {
            Some(file) => {
                ev.subject(
                    name,
                    &runtime.version,
                    Some(&runtime.artifact_sha256),
                    Source::Managed,
                );
                installs.push((package, runtime.artifact_id.clone(), file));
            }
            None => missing.push(runtime.artifact_id.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(missing);
    }
    let registry = site.open_registry();
    for (package, artifact, file) in &installs {
        install_cached_artifact(
            &site.platform,
            &site.prepared,
            &registry,
            package,
            artifact,
            file,
        )
        .unwrap_or_else(|error| panic!("{artifact} installs from the cache: {error:?}"));
    }
    Ok(())
}

/// One service of the environment: its instance and the port of each
/// endpoint.
#[derive(Debug, Clone)]
struct Member {
    instance: Uuid,
    ports: BTreeMap<String, u16>,
}

fn members_of(detail: &EnvironmentDetail) -> BTreeMap<String, Member> {
    detail
        .services
        .iter()
        .map(|service| {
            let instance = service.instance.as_ref().expect("each service has one");
            let ports = instance
                .endpoints
                .iter()
                .filter_map(|endpoint| endpoint.port.map(|port| (endpoint.name.clone(), port)))
                .collect();
            (
                service.alias.clone(),
                Member {
                    instance: instance.id,
                    ports,
                },
            )
        })
        .collect()
}

/// Whether `relative`, a file of the data folder of the service `alias`,
/// belongs to a running process only, named per provider (never by a name
/// that would match in every provider).
fn is_process_file(alias: &str, relative: &Path) -> bool {
    let name = relative
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let top = relative
        .components()
        .next()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    match alias {
        // The postmaster writes its PID file at the top of the cluster.
        "postgres" => relative == Path::new("postmaster.pid"),
        // mysqld: its PID file, the temporary tablespaces, and its sockets.
        "mysql" => {
            name.ends_with(".pid")
                || name == "ibtmp1"
                || top.starts_with("#innodb_temp")
                || name.starts_with("mysql.sock")
                || name.starts_with("mysqlx.sock")
        }
        // ClickHouse: the `status` file of a running server at the top.
        "clickhouse" => relative == Path::new("status"),
        _ => false,
    }
}

/// The SHA-256 and size of every file of the data folder of `alias`,
/// relative to it, outside its process files.
fn data_digests(site: &Site, alias: &str, instance: Uuid) -> BTreeMap<PathBuf, (String, u64)> {
    let root = site.instance_paths(instance).data;
    let mut files = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let relative = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() && !is_process_file(alias, &relative) {
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let digest: String = sha2::Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                files.insert(relative, (digest, meta.len()));
            }
        }
    }
    files
}

/// Every file of the runtime folder, relative to it, with its size.
fn runtime_tree(site: &Site) -> BTreeMap<PathBuf, u64> {
    let root = site.prepared.layout().runtimes_dir();
    let mut files = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let relative = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
            if meta.is_dir() {
                stack.push(path);
            } else {
                files.insert(relative, meta.len());
            }
        }
    }
    files
}

/// The installed client `name` (`psql`, `mysql`) in the runtime folder.
fn client(site: &Site, name: &str) -> Option<PathBuf> {
    let root = site.prepared.layout().runtimes_dir();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if entry.file_name() == name
                && path.parent().is_some_and(|parent| parent.ends_with("bin"))
            {
                return Some(path);
            }
        }
    }
    None
}

fn run(command: &mut Command) -> Result<String, String> {
    let output = command
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("cannot run: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!(
            "{}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn psql(site: &Site, port: u16, sql: &str) -> Result<String, String> {
    let psql = client(site, "psql").ok_or("no psql in the runtime")?;
    run(Command::new(psql)
        .args(["-X", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1"])
        .args(["-h", "127.0.0.1", "-p", &port.to_string()])
        .args(["-U", "postgres", "-d", "postgres", "-c", sql])
        .env("PGPASSWORD", password("postgres"))
        .env_remove("PGSERVICEFILE"))
}

fn mysql(site: &Site, port: u16, sql: &str) -> Result<String, String> {
    let mysql = client(site, "mysql").ok_or("no mysql client in the runtime")?;
    run(Command::new(mysql)
        .args(["--no-defaults", "--protocol=TCP", "-h", "127.0.0.1"])
        .arg(format!("--port={port}"))
        .args(["--user=root", "-N", "-B", "-e", sql])
        .env("MYSQL_PWD", password("mysql")))
}

/// One query over the ClickHouse HTTP interface.
fn clickhouse(port: u16, sql: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let request = format!(
        "POST / HTTP/1.0\r\nHost: 127.0.0.1\r\nX-ClickHouse-User: default\r\nX-ClickHouse-Key: {}\r\nContent-Length: {}\r\n\r\n{sql}",
        password("clickhouse"),
        sql.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    if head.starts_with("HTTP/1.0 200") || head.starts_with("HTTP/1.1 200") {
        Ok(body.trim().to_owned())
    } else {
        Err(format!("{head}\n{body}"))
    }
}

/// One RESP command to Valkey, after `AUTH default <password>`.
fn valkey(port: u16, args: &[&str]) -> Result<String, String> {
    let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut writer = stream;
    let mut send = |parts: &[&str]| -> Result<String, String> {
        let mut frame = format!("*{}\r\n", parts.len());
        for part in parts {
            frame.push_str(&format!("${}\r\n{part}\r\n", part.len()));
        }
        writer
            .write_all(frame.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let line = line.trim_end().to_owned();
        if let Some(length) = line.strip_prefix('$') {
            let length: i64 = length.parse().map_err(|_| line.clone())?;
            if length < 0 {
                return Ok(String::new());
            }
            let mut bulk = vec![0; usize::try_from(length).map_err(|_| line.clone())? + 2];
            reader.read_exact(&mut bulk).map_err(|e| e.to_string())?;
            return Ok(String::from_utf8_lossy(&bulk[..bulk.len() - 2]).into_owned());
        }
        if line.starts_with('-') {
            return Err(line);
        }
        Ok(line)
    };
    send(&["AUTH", "default", &password("valkey")])?;
    send(args)
}

/// A NATS connection that speaks just enough of the protocol for one
/// JetStream request at a time.
struct Nats {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next: u32,
}

impl Nats {
    fn connect(port: u16) -> Result<Self, String> {
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|e| e.to_string())?;
        let mut nats = Self {
            reader: BufReader::new(stream.try_clone().map_err(|e| e.to_string())?),
            writer: stream,
            next: 0,
        };
        let mut info = String::new();
        nats.reader
            .read_line(&mut info)
            .map_err(|e| e.to_string())?;
        if !info.starts_with("INFO") {
            return Err(format!("no INFO: {info}"));
        }
        let connect = format!(
            "CONNECT {{\"verbose\":false,\"pedantic\":false,\"user\":\"stackly\",\"pass\":\"{}\",\"headers\":false}}\r\nPING\r\n",
            password("nats")
        );
        nats.writer
            .write_all(connect.as_bytes())
            .map_err(|e| e.to_string())?;
        loop {
            let line = nats.line()?;
            if line == "PONG" {
                return Ok(nats);
            }
            if line.starts_with("-ERR") {
                return Err(line);
            }
        }
    }

    fn line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Ok(line.trim_end().to_owned())
    }

    /// Publishes `payload` on `subject` and returns the one reply.
    fn request(&mut self, subject: &str, payload: &str) -> Result<String, String> {
        self.next += 1;
        let inbox = format!("_INBOX.stackly.{}", self.next);
        let frame = format!(
            "SUB {inbox} {sid}\r\nPUB {subject} {inbox} {len}\r\n{payload}\r\n",
            sid = self.next,
            len = payload.len()
        );
        self.writer
            .write_all(frame.as_bytes())
            .map_err(|e| e.to_string())?;
        loop {
            let line = self.line()?;
            if line == "PING" {
                self.writer
                    .write_all(b"PONG\r\n")
                    .map_err(|e| e.to_string())?;
                continue;
            }
            if line.starts_with("-ERR") {
                return Err(line);
            }
            if let Some(rest) = line.strip_prefix("MSG ") {
                let length: usize = rest
                    .rsplit(' ')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| line.clone())?;
                let mut body = vec![0; length + 2];
                self.reader
                    .read_exact(&mut body)
                    .map_err(|e| e.to_string())?;
                return Ok(String::from_utf8_lossy(&body[..length]).into_owned());
            }
        }
    }
}

/// The base64 of `text`, without padding rules beyond the standard ones.
fn base64(text: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Writes the sentinel `token` into the service `alias`; `None` when the
/// provider keeps no data a client can write.
fn write_sentinel(
    site: &Site,
    alias: &str,
    member: &Member,
    token: &str,
) -> Option<Result<(), String>> {
    let port = |name: &str| member.ports.get(name).copied().unwrap_or_default();
    let written = match alias {
        "postgres" => psql(
            site,
            port("client"),
            &format!(
                "CREATE TABLE stackly_sentinel (v text); INSERT INTO stackly_sentinel VALUES ('{token}'); CHECKPOINT;"
            ),
        )
        .map(drop),
        "mysql" => mysql(
            site,
            port("client"),
            &format!(
                "CREATE DATABASE stackly_sentinel; CREATE TABLE stackly_sentinel.t (v varchar(64)); INSERT INTO stackly_sentinel.t VALUES ('{token}');"
            ),
        )
        .map(drop),
        "clickhouse" => clickhouse(
            port("http"),
            "CREATE TABLE default.stackly_sentinel (v String) ENGINE = MergeTree ORDER BY v",
        )
        .and_then(|_| {
            clickhouse(
                port("http"),
                &format!("INSERT INTO default.stackly_sentinel VALUES ('{token}')"),
            )
        })
        .map(drop),
        "valkey" => valkey(port("client"), &["SET", "stackly-sentinel", token]).and_then(|ok| {
            if ok == "+OK" {
                Ok(())
            } else {
                Err(ok)
            }
        }),
        "nats" => Nats::connect(port("client")).and_then(|mut nats| {
            let created = nats.request(
                "$JS.API.STREAM.CREATE.STACKLY_SENTINEL",
                "{\"name\":\"STACKLY_SENTINEL\",\"subjects\":[\"stackly.sentinel\"],\"storage\":\"file\"}",
            )?;
            if created.contains("\"error\"") {
                return Err(created);
            }
            let ack = nats.request("stackly.sentinel", token)?;
            if ack.contains("\"seq\":1") {
                Ok(())
            } else {
                Err(ack)
            }
        }),
        _ => return None,
    };
    Some(written)
}

/// Reads the sentinel of the service `alias` back.
fn read_sentinel(site: &Site, alias: &str, member: &Member) -> Result<String, String> {
    let port = |name: &str| member.ports.get(name).copied().unwrap_or_default();
    match alias {
        "postgres" => psql(site, port("client"), "SELECT v FROM stackly_sentinel"),
        "mysql" => mysql(site, port("client"), "SELECT v FROM stackly_sentinel.t"),
        "clickhouse" => clickhouse(port("http"), "SELECT v FROM default.stackly_sentinel"),
        "valkey" => valkey(port("client"), &["GET", "stackly-sentinel"]),
        "nats" => {
            let mut nats = Nats::connect(port("client"))?;
            let reply = nats.request("$JS.API.STREAM.MSG.GET.STACKLY_SENTINEL", "{\"seq\":1}")?;
            // The message data comes as base64 in `"data":"…"`.
            let data = reply
                .split("\"data\":\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .ok_or(reply.clone())?;
            Ok(data.to_owned())
        }
        other => Err(format!("{other} keeps no sentinel")),
    }
}

/// Retries `read` for a while: a server can be ready before it answers a
/// query that needs its recovered data (ClickHouse loads tables lazily).
async fn read_back(site: &Site, alias: &str, member: &Member) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let read = read_sentinel(site, alias, member);
        if read.is_ok() || tokio::time::Instant::now() > deadline {
            return read;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

// covers: AC-7, AC-26 (spec 0047), BR-006
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: brings the six official runtimes up and down from the xtask runtime cache"]
async fn down_keeps_a_sentinel_in_every_provider_and_changes_no_runtime_file() {
    let ev = evidence!(
        covers = ["REQ-022", "BR-006", "AC-022"],
        kind = Kind::Native,
        expected = "after env.up of the ERP example manifest with the six official runtimes from the xtask runtime cache, a sentinel written to PostgreSQL, MySQL, ClickHouse, Valkey, and a NATS JetStream file stream is read back from each after env.down and a second env.up; no data file outside the files only a running process owns goes away or loses its content across Down, every file a clean shutdown rewrote is named in the note, and the runtime folder keeps every file with its size through Down and the second Up",
    );
    let site = Site::new();
    let path = manifest(&site);

    let daemon = start(&site).await;
    let api = site.api().await;
    let first = plan(&api, &path).await;
    stop(daemon).await;
    assert_eq!(first.items.len(), 6, "{first:?}");
    if let Err(missing) = install_from_cache(&site, &first, &ev) {
        ev.blocked(
            &format!(
                "artifacts {} are not in the xtask runtime cache",
                missing.join(", ")
            ),
            OWNER,
        );
        return;
    }

    let daemon = start(&site).await;
    let api = site.api().await;
    let planned = plan(&api, &path).await;
    set_secrets(&api, &planned).await;
    let reply = up(&api, &path).await;
    let done = wait_final(&api, reply.operation.operation.id).await;
    let environment = reply.environment_id;
    let detail = get(&api, environment).await;
    assert_eq!(
        done.operation.state,
        OperationState::Succeeded,
        "{done:?}\n{detail:?}"
    );
    let members = members_of(&detail);
    assert_eq!(members.len(), 6);

    // A sentinel in every provider that keeps data.
    let token = format!("stackly-sentinel-{}", Uuid::now_v7().simple());
    let mut written = Vec::new();
    for (alias, member) in &members {
        match write_sentinel(&site, alias, member, &token) {
            Some(Ok(())) => written.push(alias.clone()),
            Some(Err(error)) => panic!("{alias}: the sentinel is written: {error}"),
            None => {}
        }
    }
    assert_eq!(
        written,
        ["clickhouse", "mysql", "nats", "postgres", "valkey"],
        "every provider that keeps data took a sentinel"
    );
    // Valkey writes its append only file every second.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let runtime_before = runtime_tree(&site);
    assert!(!runtime_before.is_empty(), "the runtimes are installed");
    let data_before: BTreeMap<String, BTreeMap<PathBuf, (String, u64)>> = members
        .iter()
        .map(|(alias, member)| (alias.clone(), data_digests(&site, alias, member.instance)))
        .collect();

    // Down.
    let (shown, reply) = down(&api, environment).await;
    assert_eq!(shown.stop.len(), 6, "{shown:?}");
    let done = wait_final(&api, reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let mut rewritten = Vec::new();
    for (alias, member) in &members {
        let summary = api.get(member.instance).await.expect("the instance reads");
        assert_eq!(summary.lifecycle, InstanceLifecycle::Stopped, "{alias}");
        let after = data_digests(&site, alias, member.instance);
        for (file, (digest, size)) in &data_before[alias] {
            let now = after
                .get(file)
                .unwrap_or_else(|| panic!("{alias}: {} goes away", file.display()));
            assert!(
                !(*size > 0 && now.1 == 0),
                "{alias}: {} loses its content",
                file.display()
            );
            if &now.0 != digest {
                rewritten.push(format!("{alias}/{}", file.display()));
            }
        }
    }
    assert_eq!(
        runtime_tree(&site),
        runtime_before,
        "Down changes no runtime file"
    );

    // Up again: every sentinel is still there.
    let second = up(&api, &path).await;
    let done = wait_final(&api, second.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    let again = members_of(&get(&api, environment).await);
    for alias in &written {
        let member = &again[alias];
        assert_eq!(
            member.instance, members[alias].instance,
            "{alias}: the same instance"
        );
        let read = read_back(&site, alias, member)
            .await
            .unwrap_or_else(|error| panic!("{alias}: the sentinel reads back: {error}"));
        let expected = if alias == "nats" {
            base64(&token)
        } else {
            token.clone()
        };
        assert_eq!(read, expected, "{alias}: the same sentinel");
    }
    assert_eq!(
        runtime_tree(&site),
        runtime_before,
        "the second Up changes no runtime file"
    );

    let (_, reply) = down(&api, environment).await;
    let done = wait_final(&api, reply.operation.operation.id).await;
    assert_eq!(done.operation.state, OperationState::Succeeded, "{done:?}");
    stop(daemon).await;
    rewritten.sort();
    ev.pass_with(&format!(
        "sentinels in {} read back after Down and Up; {} runtime files kept name and size through Down and the second Up; no data file went away or lost its content; files a clean shutdown rewrote: {}",
        written.join(", "),
        runtime_before.len(),
        if rewritten.is_empty() {
            "none".to_owned()
        } else {
            rewritten.join(", ")
        }
    ));
}
