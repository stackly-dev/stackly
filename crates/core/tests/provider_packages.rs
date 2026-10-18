//! The provider engine on real packages: `providers/fake` and the
//! `fake-command` fixture are copied into a `TestRoot`, read through the
//! `Filesystem` port of `FakePlatform`, validated, and planned against the
//! fixed golden context of spec 0009 ("Context golden").
//!
//! `STACKLY_UPDATE_GOLDEN=1` rewrites the golden files.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use stackly_core::provider::{
    InitStep, MarkerRead, NameCheck, PackageFiles, PlanContext, PlanError, PreflightOutcome,
    ResolvedEndpoint, RuntimeContext, Segment, ValidatedPackage, read_package,
};
use stackly_domain::{Capability, InstanceId, Port, RuntimeLine, Sha256Digest, Target};
use stackly_platform::{DirectoryLayout, RootSet};
use stackly_testkit::{FakePlatform, TestRoot};

/// The `label` of the golden fake context: every shell metacharacter.
const NASTY_LABEL: &str = "$(touch pwned); rm -rf ~ `id` \"q\" 'q' | & > ü 空";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repo root exists")
}

fn fake_dir() -> PathBuf {
    repo_root().join("providers/fake")
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

/// The fixed golden context of spec 0009 for a package.
fn golden_context(package: &ValidatedPackage, settings: serde_json::Value) -> PlanContext {
    let base = Path::new("/Volumes/Test Root/Ünï cödé");
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
    let version = "0.1.0".parse().expect("valid version");
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
            line: "0.1".parse().expect("valid line"),
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

/// Every rendered config of a plan's steps, with each secret written as
/// `<secret:field>` through its escaper.
fn rendered(steps: &[InitStep]) -> Vec<(String, String)> {
    steps
        .iter()
        .filter_map(|step| match step {
            InitStep::WriteFile(write) => {
                let bytes = write
                    .content
                    .materialize(&|secret| Ok(format!("<secret:{}>", secret.field).into_bytes()))
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
    serde_json::json!({ "label": NASTY_LABEL, "authToken": "fixture:authToken" })
}

#[test]
fn the_fake_package_reads_without_writes_and_validates() {
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
    for (config, text) in rendered(&init.steps) {
        assert_golden("fake", &format!("render-{config}.txt"), &text);
    }
}

#[test]
fn fake_command_plans_match_the_golden_files() {
    let package = validated(&fake_command_dir());
    let settings = serde_json::json!({
        "authToken": "fixture:authToken",
        "peers": [{ "name": "east", "port": 6222 }, { "name": "west" }],
    });
    let context = golden_context(&package, settings);

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
    for (config, text) in rendered(&init.steps) {
        assert_golden("fake-command", &format!("render-{config}.txt"), &text);
    }
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
    assert!(!json.contains("fixture:authToken"), "{json}");
    assert!(!debug.contains("fixture:authToken"), "{debug}");
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
    assert_eq!(secret.reference.as_str(), "fixture:authToken");
}

#[test]
fn every_argument_is_one_argv_element_as_written() {
    let package = validated(&fake_dir());
    let context = golden_context(&package, fake_settings());

    let run = package
        .run_plan(&context)
        .expect("the fake run plan builds");

    assert_eq!(run.spawn.args[5], format!("--label={NASTY_LABEL}"));
    assert_eq!(run.spawn.program.relative, Path::new("stackly-fake-server"));
    let keys: Vec<&str> = run.spawn.env.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["HOME", "PATH", "TMPDIR"]);
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
