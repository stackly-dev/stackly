//! The no shell proof of spec 0009 (AC-11; PRD AC-031, AC-058, REQ-031):
//! the fake provider's init and run plans execute `stackly-fake-server`
//! through `std::process::Command` with exactly the plan's program, argv,
//! env (after `env_clear`), cwd, and stdin. The server records the argv it
//! got, which must equal the plan, and the shell metacharacters in `label`
//! never run: no `pwned` file appears.
#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use stackly_core::provider::{
    InitStep, MarkerRead, NameCheck, PlanContext, PreflightOutcome, ResolvedEndpoint,
    RuntimeContext, Stdin, ValidatedPackage, read_package,
};
use stackly_domain::{InstanceId, Port};
use stackly_evidence::{Kind, evidence};
use stackly_platform::DirectoryLayout;
use stackly_testkit::{FakePlatform, TestRoot, allocate_port, fake_server};

/// Every shell metacharacter, as the `label` setting.
const NASTY_LABEL: &str = "$(touch pwned); rm -rf ~ `id` \"q\" 'q' | & > ü 空";
/// The fixture secret's bytes.
const TOKEN: &[u8] = b"fixture token, never a real one";

/// Kills the server if the test ends early.
struct Reap(Child);

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn package(root: &TestRoot) -> ValidatedPackage {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let copy = root.path().join("providers/fake");
    copy_dir(&source, &copy);
    let platform = FakePlatform::new(root);
    let files =
        read_package(platform.fs(), &copy).unwrap_or_else(|err| panic!("{:#?}", err.issues));
    ValidatedPackage::from_files(files, NameCheck::FolderName)
        .unwrap_or_else(|err| panic!("{:#?}", err.issues))
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("copy target");
    for entry in fs::read_dir(from).expect("listing") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy");
        }
    }
}

/// A context under the test root, with the built binary's folder as the
/// runtime root.
fn context(root: &TestRoot, package: &ValidatedPackage, binary: &Path, port: u16) -> PlanContext {
    let layout = DirectoryLayout::new(root.roots());
    let id = InstanceId::generate();
    let endpoints = package
        .endpoints()
        .map(|endpoint| {
            let resolved = ResolvedEndpoint::Ip {
                host: endpoint.default_host().expect("the fake endpoint is tcp"),
                port: Port::new(port).ok(),
            };
            (endpoint.name().clone(), resolved)
        })
        .collect();
    PlanContext {
        instance_id: id,
        instance_name: "noshell".parse().expect("valid name"),
        paths: layout.instance_paths(id),
        runtime: RuntimeContext {
            root: binary
                .parent()
                .expect("the binary has a folder")
                .to_path_buf(),
            version: "0.1.0".parse().expect("valid version"),
            line: "0.1".parse().expect("valid line"),
        },
        endpoints,
        settings: package
            .settings_schema()
            .validate(
                &serde_json::json!({ "label": NASTY_LABEL, "authToken": "fixture:authToken" }),
            )
            .unwrap_or_else(|err| panic!("{:#?}", err.issues)),
    }
}

fn command(spec: &stackly_core::provider::Command) -> Command {
    let mut command = Command::new(spec.program.absolute());
    command
        .args(&spec.args)
        .env_clear()
        .envs(spec.env.iter().map(|(key, value)| (key, value)))
        .current_dir(&spec.cwd);
    command
}

/// Runs an init plan the way the executor of spec 0010 would, minus its
/// checks.
fn run_init(init: &[InitStep]) {
    for step in init {
        match step {
            InitStep::EnsureDirectories(dirs) => {
                for dir in dirs {
                    fs::DirBuilder::new()
                        .recursive(true)
                        .mode(0o700)
                        .create(dir)
                        .expect("folder");
                }
            }
            InitStep::WriteFile(write) => {
                let bytes = write
                    .content
                    .materialize(&|_| Ok(TOKEN.to_vec()))
                    .expect("content materializes");
                fs::write(&write.path, bytes).expect("config file");
            }
            InitStep::Run(run) => {
                let mut child = command(&run.command)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("init starts");
                let mut stdin = child.stdin.take().expect("stdin is piped");
                if let Stdin::Secret(_) = run.stdin {
                    stdin.write_all(TOKEN).expect("stdin takes the token");
                }
                drop(stdin);
                let status = child.wait().expect("init exits");
                let code = status.code().and_then(|code| u8::try_from(code).ok());
                assert!(
                    code.is_some_and(|code| run.success_exit_codes.contains(&code)),
                    "init exited with {status}"
                );
            }
            InitStep::MaterializeSecret { .. } => {
                panic!("the fake provider materializes no secret file")
            }
        }
    }
}

fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

fn port_of(context: &PlanContext) -> u16 {
    context
        .endpoints
        .values()
        .find_map(|endpoint| match endpoint {
            ResolvedEndpoint::Ip { port, .. } => port.map(Port::get),
            ResolvedEndpoint::Unix => None,
        })
        .expect("the fake endpoint has a port")
}

/// Waits until `GET /ready` answers 200, or returns false when the server
/// exits first or 10 seconds pass.
fn wait_ready(server: &mut Reap, port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(response) = get(port, "/ready")
            && response.starts_with("HTTP/1.1 200")
        {
            return true;
        }
        if matches!(server.0.try_wait(), Ok(Some(_))) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        }
        found.push(path);
    }
}

#[test]
fn the_fake_plans_run_without_a_shell_and_argv_matches_the_plan() {
    let ev = evidence!(
        covers = ["REQ-031", "AC-031", "AC-058"],
        kind = Kind::Fixture,
        expected = "the fake init and run plans run through std::process::Command with a label full of shell metacharacters; argv-serve.json equals the plan's args and no pwned file appears",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let binary = fake_server::binary();
    let package = package(&root);
    let mut context = context(
        &root,
        &package,
        &binary,
        allocate_port().expect("a free loopback port"),
    );

    let init = package.init_plan(&context).expect("the init plan builds");
    run_init(&init.steps);
    let data = context.paths.data.clone();
    assert_eq!(
        fs::read_to_string(data.join("token.sha256"))
            .expect("init wrote the token digest")
            .trim(),
        Sha256::digest(TOKEN)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let marker = package
        .data_marker_path(&context.paths)
        .expect("the fake declares a marker");
    let read = MarkerRead::from_read(
        fs::read(&marker)
            .map_err(|err| stackly_platform::PlatformError::from_io("read", &marker, &err)),
    );
    assert_eq!(
        package.preflight(None, read, &context.runtime.line),
        PreflightOutcome::Compatible
    );

    // A port from allocate_port can be taken before the server binds it,
    // so a server that exits before it is ready gets a fresh port.
    let mut started = None;
    for _ in 0..3 {
        let run = package.run_plan(&context).expect("the run plan builds");
        let mut server = Reap(
            command(&run.spawn)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("serve starts"),
        );
        if wait_ready(&mut server, port_of(&context)) {
            started = Some((run, server));
            break;
        }
        let fresh = Port::new(allocate_port().expect("a free loopback port")).ok();
        for endpoint in context.endpoints.values_mut() {
            if let ResolvedEndpoint::Ip { port, .. } = endpoint {
                *port = fresh;
            }
        }
    }
    let (run, server) = started.expect("the server became ready within three ports");
    let port = port_of(&context);

    let recorded: Vec<String> = serde_json::from_slice(
        &fs::read(data.join("argv-serve.json")).expect("serve wrote its argv"),
    )
    .expect("argv is a JSON list");
    assert_eq!(recorded, run.spawn.args);
    assert!(recorded.contains(&format!("--label={NASTY_LABEL}")));
    let config = get(port, "/config").expect("GET /config answers");
    assert!(config.contains("hello from stackly"), "{config}");
    drop(server);

    let mut found = Vec::new();
    walk(root.path(), &mut found);
    walk(root.run_dir(), &mut found);
    assert!(
        found
            .iter()
            .all(|path| path.file_name().is_none_or(|name| name != "pwned")),
        "{found:#?}"
    );
    ev.pass();
}

#[test]
fn the_fixture_version_is_the_fake_server_crate_version() {
    let output = Command::new(fake_server::binary())
        .arg("version")
        .output()
        .expect("version runs");
    let versions: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake/versions.json"))
            .expect("versions.json"),
    )
    .expect("versions.json is JSON");

    let version = versions["artifacts"][0]["version"]
        .as_str()
        .expect("the fixture has a version");

    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("stackly-fake-server {version}")
    );
}
