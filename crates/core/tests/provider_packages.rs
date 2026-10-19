//! The provider engine on real packages: `providers/fake`,
//! `providers/nats`, `providers/traefik`, and the `fake-command` fixture are
//! copied into a `TestRoot`, read through the `Filesystem` port of
//! `FakePlatform`, validated, and planned against the fixed golden context
//! of spec 0009 ("Context golden").
//!
//! `STACKLY_UPDATE_GOLDEN=1` rewrites the golden files.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use stackly_core::provider::{
    EnvValue, InitStep, MarkerRead, NameCheck, PackageFiles, PlanContext, PlanError,
    PreflightOutcome, ReloadAction, ResolvedEndpoint, ResolvedSecrets, RuntimeContext, Segment,
    ValidatedPackage, read_package,
};
use stackly_domain::{
    Capability, ExactVersion, InstanceId, Port, RuntimeLine, Sha256Digest, Target,
};
use stackly_evidence::{Kind, evidence};
use stackly_platform::{DirectoryLayout, RootSet};
use stackly_testkit::{FakePlatform, TestRoot};

/// The `label` of the golden fake context: every shell metacharacter.
const NASTY_LABEL: &str = "$(touch pwned); rm -rf ~ `id` \"q\" 'q' | & > ü 空";
/// The secret reference of the golden context, in the daemon's form for the
/// golden instance (spec 0011).
const GOLDEN_SECRET_REF: &str = "local:instance/0192f000-0000-7000-8000-00000000000a/auth-token";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repo root exists")
}

fn fake_dir() -> PathBuf {
    repo_root().join("providers/fake")
}

fn nats_dir() -> PathBuf {
    repo_root().join("providers/nats")
}

fn traefik_dir() -> PathBuf {
    repo_root().join("providers/traefik")
}

fn fake_command_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/fake-command")
}

/// Copies a folder tree with `std::fs`, as test setup.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the copy target can be created");
    for entry in fs::read_dir(from).expect("the package folder can be listed") {
        let entry = entry.expect("the entry can be read");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the entry has a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("the file can be copied");
        }
    }
}

/// Copies a package into a fresh test root and reads it through the fake
/// filesystem, checking that reading changed nothing.
fn read_copy(source: &Path) -> (TestRoot, PackageFiles) {
    let root = TestRoot::new().expect("a test root can be created");
    let name = source.file_name().expect("the package has a folder name");
    let copy = root.path().join("providers").join(name);
    copy_dir(source, &copy);
    let platform = FakePlatform::new(&root);
    let before = platform.fs().mutations();

    let files =
        read_package(platform.fs(), &copy).unwrap_or_else(|err| panic!("{err}: {:#?}", err.issues));

    assert_eq!(
        platform.fs().mutations(),
        before,
        "reading a package writes nothing"
    );
    assert_eq!(platform.fs().violations(), 0);
    (root, files)
}

fn validated(source: &Path) -> ValidatedPackage {
    let (_root, files) = read_copy(source);
    ValidatedPackage::from_files(files, NameCheck::FolderName).unwrap_or_else(|err| {
        let lines: Vec<String> = err.issues.iter().map(ToString::to_string).collect();
        panic!("{err}:\n{}", lines.join("\n"))
    })
}

/// The version and line of the package's first artifact, which name the
/// runtime folder of the golden context.
fn first_artifact(package: &ValidatedPackage) -> (ExactVersion, RuntimeLine) {
    let versions: serde_json::Value = serde_json::from_slice(
        package
            .files()
            .get("versions.json")
            .expect("every package has versions.json"),
    )
    .expect("versions.json is JSON");
    let artifact = &versions["artifacts"][0];
    let text = |field: &str| artifact[field].as_str().expect("the field is text");
    (
        text("version").parse().expect("valid version"),
        text("line").parse().expect("valid line"),
    )
}

/// The fixed golden context of spec 0009 for a package.
fn golden_context(package: &ValidatedPackage, settings: serde_json::Value) -> PlanContext {
    context_under(Path::new("/Volumes/Test Root/Ünï cödé"), package, settings)
}

/// The golden context with the data and logs roots under `base`.
fn context_under(
    base: &Path,
    package: &ValidatedPackage,
    settings: serde_json::Value,
) -> PlanContext {
    let roots = RootSet::new(
        base.join("data"),
        base.join("logs"),
        base.join("cache"),
        PathBuf::from("/private/tmp/skt-golden/stackly-0000000a"),
    )
    .expect("the golden roots are valid");
    let layout = DirectoryLayout::new(roots);
    let id: InstanceId = "0192f000-0000-7000-8000-00000000000a"
        .parse()
        .expect("the golden ID is a UUID v7");
    let (version, line) = first_artifact(package);
    let zeros: Sha256Digest = "0".repeat(64).parse().expect("64 zeros are a digest");
    let mut port = 14000;
    let mut endpoints = BTreeMap::new();
    for endpoint in package.endpoints() {
        let resolved = match endpoint.default_host() {
            Some(host) => {
                let resolved = ResolvedEndpoint::Ip {
                    host,
                    port: Some(Port::new(port).expect("golden ports are not zero")),
                };
                port += 1;
                resolved
            }
            None => ResolvedEndpoint::Unix,
        };
        endpoints.insert(endpoint.name().clone(), resolved);
    }
    PlanContext {
        instance_id: id,
        instance_name: "golden".parse().expect("valid name"),
        paths: layout.instance_paths(id),
        runtime: RuntimeContext {
            root: layout.runtime_dir(
                package.software(),
                &version,
                "macos-aarch64".parse::<Target>().expect("valid target"),
                &zeros,
            ),
            version,
            line,
        },
        endpoints,
        settings: package
            .settings_schema()
            .validate(&settings)
            .unwrap_or_else(|err| panic!("golden settings are valid: {:#?}", err.issues)),
    }
}

/// Compares `actual` with a golden file, or rewrites it when
/// `STACKLY_UPDATE_GOLDEN=1`.
fn assert_golden(package: &str, name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/provider")
        .join(package)
        .join(name);
    if std::env::var("STACKLY_UPDATE_GOLDEN").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().expect("golden files have a folder"))
            .expect("the golden folder can be created");
        fs::write(&path, actual).expect("the golden file can be written");
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "{}: {err}; run with STACKLY_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    assert_eq!(
        actual,
        expected,
        "{} differs from the golden file",
        path.display()
    );
}

fn pretty(value: &impl serde::Serialize) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("plans serialize");
    text.push('\n');
    text
}

/// Every secret field of a package resolved to the text `<secret:field>`.
fn placeholder_secrets(package: &ValidatedPackage) -> ResolvedSecrets {
    ResolvedSecrets::for_tests(
        package
            .settings_schema()
            .fields()
            .filter(|field| field.is_secret())
            .map(|field| {
                (
                    field.name().clone(),
                    format!("<secret:{}>", field.name()).into_bytes(),
                )
            }),
    )
}

/// Every rendered config of a plan's steps, with each secret written as
/// `<secret:field>` through its escaper.
fn rendered(package: &ValidatedPackage, steps: &[InitStep]) -> Vec<(String, String)> {
    let secrets = placeholder_secrets(package);
    steps
        .iter()
        .filter_map(|step| match step {
            InitStep::WriteFile(write) => {
                let bytes = write
                    .content
                    .materialize(&secrets)
                    .expect("golden content materializes");
                Some((
                    write.content.config_file.clone(),
                    String::from_utf8(bytes).expect("rendered text is UTF-8"),
                ))
            }
            _ => None,
        })
        .collect()
}

fn fake_settings() -> serde_json::Value {
    serde_json::json!({ "label": NASTY_LABEL, "authToken": GOLDEN_SECRET_REF })
}

fn fake_command_settings() -> serde_json::Value {
    serde_json::json!({
        "authToken": GOLDEN_SECRET_REF,
        "peers": [{ "name": "east", "port": 6222 }, { "name": "west" }],
    })
}

#[test]
fn the_fake_package_reads_without_writes_and_validates() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "providers/fake copied into a test root reads through the Filesystem port without a write and validates",
    );
    let package = validated(&fake_dir());

    let key = package.key();
    assert_eq!(key.provider().as_str(), "testing/fake");
    assert_eq!(key.revision().as_str(), "0.1.0");
    assert_eq!(package.software().as_str(), "fake");
    assert_eq!(
        package
            .provider_revision()
            .capabilities()
            .iter()
            .collect::<Vec<_>>(),
        [
            Capability::Reload,
            Capability::ColdBackup,
            Capability::MultiInstance,
            Capability::Persistence
        ]
    );
    assert_eq!(package.provider_revision().key(), key);
    ev.pass();
}

#[test]
fn the_fake_command_fixture_validates_with_the_primitives_fake_does_not_use() {
    let package = validated(&fake_command_dir());

    assert_eq!(package.key().provider().as_str(), "testing/fake-command");
    let line = |text: &str| text.parse::<RuntimeLine>().expect("valid line");
    assert!(
        package
            .provider_revision()
            .line_compatibility()
            .allows(&line("0.1"), &line("0.2"))
    );
    assert_eq!(
        package
            .provider_revision()
            .capabilities()
            .iter()
            .collect::<Vec<_>>(),
        [Capability::Reload, Capability::Persistence]
    );
}

#[test]
fn fake_plans_match_the_golden_files() {
    let package = validated(&fake_dir());
    let context = golden_context(&package, fake_settings());

    let init = package
        .init_plan(&context)
        .expect("the fake init plan builds");
    let run = package
        .run_plan(&context)
        .expect("the fake run plan builds");
    let reload = package
        .reload_plan(&context)
        .expect("the fake reload plan builds");

    assert_golden("fake", "init.json", &pretty(&init));
    assert_golden("fake", "run.json", &pretty(&run));
    assert_golden("fake", "reload.json", &pretty(&reload));
    for (config, text) in rendered(&package, &init.steps) {
        assert_golden("fake", &format!("render-{config}.txt"), &text);
    }
}

#[test]
fn fake_command_plans_match_the_golden_files() {
    let package = validated(&fake_command_dir());
    let context = golden_context(&package, fake_command_settings());

    let init = package
        .init_plan(&context)
        .expect("the fake-command init plan builds");
    let run = package
        .run_plan(&context)
        .expect("the fake-command run plan builds");
    let reload = package
        .reload_plan(&context)
        .expect("the fake-command reload plan builds");

    assert_golden("fake-command", "init.json", &pretty(&init));
    assert_golden("fake-command", "run.json", &pretty(&run));
    assert_golden("fake-command", "reload.json", &pretty(&reload));
    for (config, text) in rendered(&package, &init.steps) {
        assert_golden("fake-command", &format!("render-{config}.txt"), &text);
    }
}

#[test]
fn materialize_secret_names_a_file_in_the_secrets_folder() {
    let package = validated(&fake_command_dir());
    let context = golden_context(&package, fake_command_settings());
    let token = context.paths.run.join("secrets/token");

    let init = package
        .init_plan(&context)
        .expect("the fake-command init plan builds");

    let (secret, file_name) = init
        .steps
        .iter()
        .find_map(|step| match step {
            InitStep::MaterializeSecret { secret, file_name } => Some((secret, file_name)),
            _ => None,
        })
        .expect("fake-command materializes its token");
    assert_eq!(secret.field.as_str(), "authToken");
    assert_eq!(file_name, "token");
    assert_eq!(init.cleanup, [token.as_path()]);
    let run = init
        .steps
        .iter()
        .find_map(|step| match step {
            InitStep::Run(run) => Some(run),
            _ => None,
        })
        .expect("fake-command runs init");
    assert!(
        run.command.env.contains(&(
            "FAKE_TOKEN_FILE".to_owned(),
            EnvValue::Text(token.to_str().expect("UTF-8 path").to_owned())
        )),
        "{:?}",
        run.command.env
    );
}

#[test]
fn a_whole_secret_env_value_of_the_start_command_stays_a_secret() {
    let package = validated(&fake_command_dir());
    let context = golden_context(&package, fake_command_settings());

    let run = package
        .run_plan(&context)
        .expect("the fake-command run plan builds");

    let (_, value) = run
        .spawn
        .env
        .iter()
        .find(|(key, _)| key == "FAKE_TOKEN")
        .expect("the start command sets FAKE_TOKEN");
    let EnvValue::Secret(secret) = value else {
        panic!("FAKE_TOKEN should be a secret, got {value:?}");
    };
    assert_eq!(secret.field.as_str(), "authToken");
    assert_eq!(secret.reference.as_str(), GOLDEN_SECRET_REF);
    let json = serde_json::to_string(&run).expect("plans serialize");
    let debug = format!("{run:?}");
    assert!(json.contains("<secret:authToken>"), "{json}");
    assert!(!json.contains(GOLDEN_SECRET_REF), "{json}");
    assert!(!debug.contains(GOLDEN_SECRET_REF), "{debug}");
}

#[test]
fn run_plans_render_every_config_file_before_the_spawn() {
    let package = validated(&fake_command_dir());
    let context = golden_context(&package, fake_command_settings());
    let config = &context.paths.config;

    let run = package
        .run_plan(&context)
        .expect("the fake-command run plan builds");

    let [
        InitStep::EnsureDirectories(dirs),
        InitStep::WriteFile(peers),
        InitStep::WriteFile(server),
    ] = run.render.as_slice()
    else {
        panic!("one EnsureDirectories and two writes: {:#?}", run.render);
    };
    assert_eq!(dirs, &[config.clone(), config.join("conf")]);
    assert_eq!(peers.path, config.join("peers.yaml"));
    assert_eq!(server.path, config.join("conf/server.json"));
    assert!(server.sensitive);
    assert_eq!(
        rendered(&package, &run.render),
        rendered(
            &package,
            &package
                .init_plan(&context)
                .expect("the fake-command init plan builds")
                .steps
        )
        .into_iter()
        .rev()
        .collect::<Vec<_>>(),
        "the run plan renders the same files as initialize, sorted by name"
    );
}

#[test]
fn plans_show_secrets_only_as_their_field() {
    let package = validated(&fake_dir());
    let context = golden_context(&package, fake_settings());

    let init = package
        .init_plan(&context)
        .expect("the fake init plan builds");
    let json = serde_json::to_string(&init).expect("plans serialize");
    let debug = format!("{init:?}");

    assert!(json.contains("<secret:authToken>"), "{json}");
    assert!(!json.contains(GOLDEN_SECRET_REF), "{json}");
    assert!(!debug.contains(GOLDEN_SECRET_REF), "{debug}");
    let secret = init
        .steps
        .iter()
        .find_map(|step| match step {
            InitStep::WriteFile(write) => {
                write
                    .content
                    .segments
                    .iter()
                    .find_map(|segment| match segment {
                        Segment::Secret { secret, .. } => Some(secret.clone()),
                        Segment::Text(_) => None,
                    })
            }
            _ => None,
        })
        .expect("the fake config holds the auth token");
    assert_eq!(secret.reference.as_str(), GOLDEN_SECRET_REF);
}

#[test]
fn every_argument_is_one_argv_element_as_written() {
    let ev = evidence!(
        covers = ["REQ-031", "AC-031"],
        kind = Kind::Fixture,
        expected = "a settings value full of shell metacharacters stays one argv element exactly as written, and the child env holds only HOME, PATH, and TMPDIR",
    );
    let package = validated(&fake_dir());
    let context = golden_context(&package, fake_settings());

    let run = package
        .run_plan(&context)
        .expect("the fake run plan builds");

    assert_eq!(run.spawn.args[5], format!("--label={NASTY_LABEL}"));
    assert_eq!(run.spawn.program.relative, Path::new("stackly-fake-server"));
    let keys: Vec<&str> = run.spawn.env.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["HOME", "PATH", "TMPDIR"]);
    ev.pass();
}

#[test]
fn reload_of_a_provider_without_reload_is_unsupported() {
    let source = fake_dir();
    let (_root, files) = read_copy(&source);
    let text =
        String::from_utf8(files.get("provider.yaml").expect("manifest").to_vec()).expect("UTF-8");
    let without = text
        .replace("  reload: true\n", "")
        .replace("  reload:\n    render: [fakeConfig]\n    action: signal\n    signal: hup\n    verify: readiness\n", "")
        .replace("  reloadSeconds: 10\n", "");
    let schema = String::from_utf8(
        files
            .get("schemas/settings.schema.json")
            .expect("schema")
            .to_vec(),
    )
    .expect("UTF-8")
    .replace("\"apply\": \"reload\"", "\"apply\": \"restart\"");
    let mut map: BTreeMap<String, Vec<u8>> = files
        .iter()
        .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
        .collect();
    map.insert("provider.yaml".into(), without.into_bytes());
    map.insert("schemas/settings.schema.json".into(), schema.into_bytes());
    let package =
        ValidatedPackage::from_files(PackageFiles::new("fake", map), NameCheck::FolderName)
            .unwrap_or_else(|err| panic!("{:#?}", err.issues));
    let context = golden_context(&package, fake_settings());

    let err = package.reload_plan(&context).unwrap_err();

    assert_eq!(
        err,
        PlanError::Unsupported {
            capability: Capability::Reload
        }
    );
    assert!(err.rule().is_some());
}

#[test]
fn preflight_reads_the_fake_data_marker() {
    let package = validated(&fake_dir());
    let context = golden_context(&package, fake_settings());
    let line = |text: &str| text.parse::<RuntimeLine>().expect("valid line");

    assert_eq!(
        package.data_marker_path(&context.paths),
        Some(context.paths.data.join("FAKE_VERSION"))
    );
    assert_eq!(
        package.preflight(
            Some(line("0.1")),
            MarkerRead::Contents("0.1\n".into()),
            &line("0.1")
        ),
        PreflightOutcome::Compatible
    );
    assert_eq!(
        package.preflight(Some(line("0.1")), MarkerRead::Missing, &line("0.1")),
        PreflightOutcome::MarkerMissing
    );
    assert_eq!(
        package.preflight(None, MarkerRead::Unreadable, &line("0.1")),
        PreflightOutcome::MarkerUnreadable
    );
    assert_eq!(
        package.preflight(
            Some(line("0.1")),
            MarkerRead::Contents("0.2".into()),
            &line("0.1")
        ),
        PreflightOutcome::MarkerMismatch {
            recorded: Some(line("0.1")),
            marker: Some(line("0.2")),
        }
    );
    assert_eq!(
        package.preflight(None, MarkerRead::Contents("0.1".into()), &line("0.2")),
        PreflightOutcome::Incompatible {
            data: line("0.1"),
            runtime: line("0.2"),
        }
    );
}

#[test]
fn the_nats_package_reads_without_writes_and_validates() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "providers/nats copied into a test root reads through the Filesystem port without a write and validates",
    );
    let package = validated(&nats_dir());

    let key = package.key();
    assert_eq!(key.provider().as_str(), "official/nats");
    assert_eq!(key.revision().as_str(), "1.0.0");
    assert_eq!(package.software().as_str(), "nats");
    assert_eq!(
        package
            .provider_revision()
            .capabilities()
            .iter()
            .collect::<Vec<_>>(),
        [
            Capability::ColdBackup,
            Capability::Metrics,
            Capability::MultiInstance,
            Capability::Persistence
        ]
    );
    assert_eq!(
        package.data_marker_path(&golden_context(&package, serde_json::json!({})).paths),
        None
    );
    ev.pass();
}

#[test]
fn nats_plans_match_the_golden_files() {
    let package = validated(&nats_dir());
    let context = golden_context(&package, serde_json::json!({}));

    let init = package
        .init_plan(&context)
        .expect("the nats init plan builds");
    let run = package
        .run_plan(&context)
        .expect("the nats run plan builds");

    assert_golden("nats", "init.json", &pretty(&init));
    assert_golden("nats", "run.json", &pretty(&run));
    let renders = rendered(&package, &init.steps);
    assert_eq!(renders.len(), 1);
    for (config, text) in renders {
        assert_golden("nats", &format!("render-{config}.txt"), &text);
    }
}

#[test]
fn nats_without_jetstream_renders_no_jetstream_block() {
    let package = validated(&nats_dir());
    let context = golden_context(&package, serde_json::json!({ "jetstream": false }));

    let init = package
        .init_plan(&context)
        .expect("the nats init plan builds");

    let (_, text) = rendered(&package, &init.steps)
        .into_iter()
        .next()
        .expect("nats renders nats.conf");
    assert!(!text.contains("jetstream"), "{text}");
    assert!(text.contains("max_payload: 1048576\n"), "{text}");
}

#[test]
fn reload_of_nats_is_unsupported() {
    let package = validated(&nats_dir());
    let context = golden_context(&package, serde_json::json!({}));

    assert_eq!(
        package.reload_plan(&context).unwrap_err(),
        PlanError::Unsupported {
            capability: Capability::Reload
        }
    );
}

#[test]
fn a_quote_in_the_data_folder_fails_the_nats_plan_without_the_value() {
    let package = validated(&nats_dir());
    let context = context_under(
        Path::new("/Users/o\"q/Library/Application Support/Stackly"),
        &package,
        serde_json::json!({}),
    );

    let err = package.init_plan(&context).unwrap_err();

    let PlanError::UnrepresentableValue { field } = &err else {
        panic!("expected UnrepresentableValue, got {err:?}");
    };
    assert!(field.contains("instance.data"), "{field}");
    assert!(!err.to_string().contains("o\"q"), "{err}");
}

/// The first `count` of the three example routes of the golden context.
fn traefik_settings(count: usize) -> serde_json::Value {
    let routes = [
        serde_json::json!({
            "name": "api",
            "rule": "Host(`api.localhost`)",
            "backendUrl": "http://127.0.0.1:3000",
        }),
        serde_json::json!({
            "name": "web-app",
            "rule": "Host(`app.localhost`) && PathPrefix(`/v1`)",
            "backendUrl": "http://localhost:5173/",
        }),
        serde_json::json!({
            "name": "docs",
            "rule": "Host(\"docs.localhost\") || PathPrefix(`/docs`)",
            "backendUrl": "http://[::1]:8080/docs",
        }),
    ];
    serde_json::json!({ "routes": routes[..count] })
}

#[test]
fn the_traefik_package_reads_without_writes_and_validates() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "providers/traefik with its routes list copied into a test root reads through the Filesystem port without a write and validates",
    );
    let package = validated(&traefik_dir());

    let key = package.key();
    assert_eq!(key.provider().as_str(), "official/traefik");
    assert_eq!(key.revision().as_str(), "1.0.0");
    assert_eq!(package.software().as_str(), "traefik");
    assert_eq!(
        package
            .provider_revision()
            .capabilities()
            .iter()
            .collect::<Vec<_>>(),
        [Capability::Reload, Capability::MultiInstance]
    );
    ev.pass();
}

#[test]
fn traefik_plans_with_three_routes_match_the_golden_files() {
    let package = validated(&traefik_dir());
    let context = golden_context(&package, traefik_settings(3));

    let init = package
        .init_plan(&context)
        .expect("the traefik init plan builds");
    let run = package
        .run_plan(&context)
        .expect("the traefik run plan builds");
    let reload = package
        .reload_plan(&context)
        .expect("the traefik reload plan builds");

    assert_golden("traefik", "init.json", &pretty(&init));
    assert_golden("traefik", "run.json", &pretty(&run));
    assert_golden("traefik", "reload.json", &pretty(&reload));
    let renders = rendered(&package, &init.steps);
    assert_eq!(renders.len(), 2);
    for (config, text) in renders {
        assert_golden("traefik", &format!("render-{config}.txt"), &text);
    }
}

#[test]
fn traefik_routes_render_for_zero_one_and_three_routes() {
    let package = validated(&traefik_dir());

    // Three routes is the golden context, so its routes file is the one
    // the init plan renders.
    for (count, golden) in [
        (0, "routes-zero.txt"),
        (1, "routes-one.txt"),
        (3, "render-routes.txt"),
    ] {
        let context = golden_context(&package, traefik_settings(count));
        let reload = package
            .reload_plan(&context)
            .expect("the traefik reload plan builds");

        let renders = rendered(&package, &reload.steps);
        let [(config, text)] = renders.as_slice() else {
            panic!("a traefik reload renders only routes: {renders:?}");
        };
        assert_eq!(config, "routes");
        assert_golden("traefik", golden, text);
        let parsed: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(text).expect("the routes file is YAML");
        let routers = parsed["http"]["routers"]
            .as_mapping()
            .map_or(0, |map| map.len());
        assert_eq!(routers, count, "{text}");
    }
}

#[test]
fn a_traefik_reload_rewrites_only_routes_and_verifies_readiness() {
    let package = validated(&traefik_dir());
    let context = golden_context(&package, traefik_settings(1));

    let reload = package
        .reload_plan(&context)
        .expect("the traefik reload plan builds");

    assert_eq!(reload.action, ReloadAction::None);
    let verify = reload
        .verify
        .expect("traefik verifies readiness after reload");
    assert_eq!(verify.deadline, std::time::Duration::from_secs(30));
    let dynamic = context.paths.config.join("dynamic");
    assert!(
        reload.steps.iter().any(
            |step| matches!(step, InitStep::EnsureDirectories(dirs) if dirs.contains(&dynamic))
        ),
        "the reload plan creates config/dynamic before writing routes"
    );
}
