//! Fuzz harnesses of the provider engine (spec 0009, *Fuzz target*). The
//! cargo-fuzz targets in `fuzz/` and the stable regression tests in
//! `crates/core/tests/fuzz_regressions.rs` call these same functions. Each
//! one panics when an invariant breaks, which is what the fuzzer reports as
//! a crash.
//!
//! No harness touches a filesystem: packages are built in memory from the
//! example packages embedded at compile time, and validated with
//! [`ValidatedPackage::from_files`] or the parser of one module.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::sync::OnceLock;
use std::time::Duration;

use stackly_domain::EndpointName;
use stackly_platform::SecretValue;

use super::interpolate::{
    EndpointTransport, InstanceDir, PathBase, PathPosition, Piece, RawPiece, Token, TypeEnv,
    ValuePosition, check_executable, check_literal, check_path, check_secret_file_name,
    check_socket_name, check_value, parse,
};
use super::issue::{FileIssues, IssueCode, Issues, MAX_ISSUES};
use super::names::{is_component, is_component_byte};
use super::package::{MANIFEST_FILE, NameCheck, PackageFiles};
use super::plan::{
    ARGUMENT_MAX_LEN, CHILD_PATH, Command, EnvValue, InitStep, PlanContext, PlanError, ProbeSpec,
    ReloadAction, Resolver, StopPlan,
};
use super::probe::NativeProbeFailure;
use super::probe::nats::{self, Exchange, Expectation, connect_bytes};
use super::raw::{RawSettingsSchema, decode_json, parse_json};
use super::settings::{SettingsSchema, parse_schema};
use super::validate::{ValidatedPackage, sample_context};
use crate::secret::ResolvedSecrets;

/// `providers/fake`, embedded.
const FAKE: [(&str, &str); 4] = [
    (
        "provider.yaml",
        include_str!("../../../../providers/fake/provider.yaml"),
    ),
    (
        "versions.json",
        include_str!("../../../../providers/fake/versions.json"),
    ),
    (
        "schemas/settings.schema.json",
        include_str!("../../../../providers/fake/schemas/settings.schema.json"),
    ),
    (
        "templates/fake.json.tmpl",
        include_str!("../../../../providers/fake/templates/fake.json.tmpl"),
    ),
];

/// `providers/traefik`, embedded, for templates that loop over a list.
const TRAEFIK: [(&str, &str); 5] = [
    (
        "provider.yaml",
        include_str!("../../../../providers/traefik/provider.yaml"),
    ),
    (
        "versions.json",
        include_str!("../../../../providers/traefik/versions.json"),
    ),
    (
        "schemas/settings.schema.json",
        include_str!("../../../../providers/traefik/schemas/settings.schema.json"),
    ),
    (
        "templates/traefik.yaml.tmpl",
        include_str!("../../../../providers/traefik/templates/traefik.yaml.tmpl"),
    ),
    (
        "templates/routes.yaml.tmpl",
        include_str!("../../../../providers/traefik/templates/routes.yaml.tmpl"),
    ),
];

/// The fake template, the one `provider_template` replaces in the fake
/// package.
const FAKE_TEMPLATE: &str = "templates/fake.json.tmpl";
/// The traefik template that loops over `settings.routes`.
const TRAEFIK_ROUTES: &str = "templates/routes.yaml.tmpl";
/// The value every secret resolves to in a harness: a quote, a backslash,
/// a dollar, a newline, and a space, so each escaper has work to do.
const AWKWARD_SECRET: &str = "s\"e\\c$r\ne t";
/// A secret reference in the daemon's form (spec 0011), for the documents
/// of the settings harness.
const SAMPLE_SECRET_REF: &str = "local:instance/0192f000-0000-7000-8000-000000000001/auth-token";

/// Target `provider_manifest`: `data` as `provider.yaml` next to the fake
/// package's settings schema, template, and `versions.json`.
///
/// # Panics
///
/// Panics when a rejected package has no issue or too many, or when a
/// valid package breaks an invariant of [`exercise`].
pub fn manifest(data: &[u8]) {
    let mut files = package_files(&FAKE);
    files.insert(MANIFEST_FILE.to_owned(), data.to_vec());
    check_package(PackageFiles::new("fake", files));
}

/// Target `provider_template`: the first byte picks a setup, the rest is
/// the template. Setups 0, 1, 2, 4, and 5 replace the fake template and
/// render it as `json`, `yaml`, `natsConf`, `postgresqlConf`, and `plain`
/// against the fake context; setup 3 replaces the traefik routes template,
/// whose context has a list. Setups 0 to 3 keep the numbers of the seeds
/// written before the `postgresqlConf` and `plain` formats (spec 0019).
///
/// # Panics
///
/// Panics when a rejected package has no issue or too many, or when a
/// valid template renders output its format cannot parse, or breaks
/// another invariant of [`exercise`].
pub fn template(data: &[u8]) {
    let Some((&selector, template)) = data.split_first() else {
        return;
    };
    let (folder, mut files, slot) = match selector % 6 {
        3 => ("traefik", package_files(&TRAEFIK), TRAEFIK_ROUTES),
        other => {
            let mut files = package_files(&FAKE);
            let format = match other {
                0 => "json",
                1 => "yaml",
                2 => "natsConf",
                4 => "postgresqlConf",
                _ => "plain",
            };
            let manifest = FAKE[0]
                .1
                .replace("format: json", &format!("format: {format}"));
            files.insert(MANIFEST_FILE.to_owned(), manifest.into_bytes());
            ("fake", files, FAKE_TEMPLATE)
        }
    };
    files.insert(slot.to_owned(), template.to_vec());
    check_package(PackageFiles::new(folder, files));
}

/// Target `provider_interpolation`: `data` as the text of a manifest field,
/// checked in every kind of position against the fake package and
/// rendered with its sample context.
///
/// # Panics
///
/// Panics when an accepted string breaks the rule of its position: a
/// secret in a value other than a whole env value of a command that
/// resolves secrets, a flag from a free setting, an executable with text
/// around it, a path component that escapes its folder, the secrets folder
/// outside a sensitive destination, a sensitive destination outside the
/// secrets folder (so a destination has the secrets folder as its base if
/// and only if it is sensitive), a secret file name that is not one
/// component, or a socket name longer than 15 bytes.
pub fn interpolation(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let package = fake_package();
    let manifest = &package.manifest;
    let endpoints: BTreeMap<EndpointName, EndpointTransport> = manifest
        .endpoints
        .iter()
        .map(|(name, spec)| (name.clone(), spec.transport))
        .collect();
    let aliases = BTreeSet::new();
    let managed_fs: BTreeSet<InstanceDir> = InstanceDir::ALL.into_iter().collect();
    let env = TypeEnv {
        endpoints: &endpoints,
        aliases: &aliases,
        settings: Some(&package.settings),
        managed_fs: &managed_fs,
    };
    let context = sample_context(manifest, &package.settings, true)
        .expect("the fake package has a sample context");
    let resolver = Resolver::new(manifest, &context).expect("the fake sample context resolves");
    let mut issues = Issues::new();
    let mut sink = FileIssues::new(MANIFEST_FILE, &mut issues);
    let parsed = parse(text);

    for position in [
        ValuePosition::Arg,
        ValuePosition::EnvValue { secrets: false },
        ValuePosition::EnvValue { secrets: true },
        ValuePosition::Managed,
    ] {
        let Some(interp) = check_value(text, position, &env, &mut sink, "fuzz") else {
            continue;
        };
        assert!(parsed.is_ok(), "an accepted value parses");
        let has_secret = interp
            .tokens()
            .any(|token| matches!(token, Token::Secret(_)));
        if position == (ValuePosition::EnvValue { secrets: true }) {
            assert!(
                !has_secret || matches!(interp.pieces.as_slice(), [Piece::Token(Token::Secret(_))]),
                "a secret env value is not one whole token"
            );
        } else {
            assert!(!has_secret, "a secret was accepted in a value position");
        }
        if position == ValuePosition::Managed {
            assert!(
                !interp
                    .tokens()
                    .any(|token| matches!(token, Token::InstanceSecrets)),
                "a managed expression names the secrets folder"
            );
        }
        if position == ValuePosition::Arg
            && let Some(Piece::Token(Token::Setting(name))) = interp.pieces.first()
        {
            let field = package
                .settings
                .field(name.as_str())
                .expect("an accepted setting token names a field");
            assert!(
                !field.is_free_user_value(),
                "an argument starts with a free user value"
            );
        }
        // Rendering may fail on data; it must not panic.
        let _ = resolver.render(&interp, "fuzz");
    }
    if check_executable(text, &env, &mut sink, "fuzz").is_some() {
        assert!(
            matches!(parsed.as_deref(), Ok([RawPiece::Token(_)])),
            "an executable is exactly one token"
        );
    }
    for position in [
        PathPosition::Cwd,
        PathPosition::ConfigDestination { sensitive: false },
        PathPosition::ConfigDestination { sensitive: true },
        PathPosition::LogFile,
    ] {
        let Some(expr) = check_path(text, position, &env, &mut sink, "fuzz") else {
            continue;
        };
        assert!(
            expr.components
                .iter()
                .all(|component| is_component(component)),
            "a path component breaks the charset or is . or .."
        );
        let is_destination = matches!(position, PathPosition::ConfigDestination { .. });
        let is_sensitive_destination =
            position == (PathPosition::ConfigDestination { sensitive: true });
        assert!(
            expr.base != PathBase::Secrets || is_sensitive_destination,
            "the secrets folder starts a path that is not a sensitive destination"
        );
        assert!(
            !is_destination || (expr.base == PathBase::Secrets) == is_sensitive_destination,
            "a sensitive destination is outside the secrets folder, or a plain one is inside it"
        );
        let base = expr.base.path(&context.paths);
        let full = expr
            .components
            .iter()
            .fold(base.clone(), |path, component| path.join(component));
        assert!(full.starts_with(&base), "a path left its folder");
        assert!(
            full.components()
                .all(|component| !matches!(component, Component::ParentDir | Component::CurDir)),
            "a path holds . or .."
        );
    }
    if let Some(name) = check_secret_file_name(text, &env, &mut sink, "fuzz") {
        assert!(
            is_component(&name),
            "a secret file name is not one component"
        );
    }
    if let Some(interp) = check_socket_name(text, &env, &mut sink, "fuzz")
        && let Ok(name) = resolver.render(&interp, "fuzz")
    {
        assert!(
            (1..=15).contains(&name.len())
                && name.bytes().all(is_component_byte)
                && name != "."
                && name != "..",
            "socket name {name:?} breaks SocketName"
        );
    }
    if let Some(literal) = check_literal(text, &mut sink, "fuzz") {
        assert!(
            parsed.is_ok_and(|pieces| pieces
                .iter()
                .all(|piece| matches!(piece, RawPiece::Literal(_)))),
            "a literal field accepted a token"
        );
        assert!(literal.len() <= text.len(), "unescaping grew the text");
    }
}

/// Target `provider_settings_schema`: `data` as a settings schema, parsed
/// in the dialect, then a few fixed documents validated against it.
///
/// # Panics
///
/// Panics when validation accepts a document but leaves a plain field
/// without a value, keeps a managed field, or does not accept its own
/// output again unchanged, or when a rejection has no issue.
pub fn settings_schema(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let mut issues = Issues::new();
    let Some(schema) = parse_settings(text, &mut issues) else {
        assert!(!issues.is_empty(), "a rejected schema has an issue");
        return;
    };
    for document in documents(&schema) {
        match schema.validate(&document) {
            Ok(values) => {
                for field in schema.fields() {
                    let value = values.get(field.name().as_str());
                    if field.managed().is_some() {
                        assert!(value.is_none(), "a managed field kept a value");
                    } else if !field.is_secret() {
                        assert!(value.is_some(), "{} has no value", field.name());
                    }
                }
                let again = schema
                    .validate(&values.to_json())
                    .expect("validated values validate again");
                assert_eq!(again, values, "validated values changed on a second pass");
            }
            Err(err) => assert!(!err.issues.is_empty(), "a rejected document has an issue"),
        }
    }
}

/// The files of an embedded package.
/// Target `nats_readiness_protocol` (spec 0023, *Fuzz target*): bit 0 of
/// the first byte picks the JetStream expectation, the first byte shifted
/// right by one plus one (1 to 128) is the chunk size, and the other bytes
/// reach [`Exchange`] as the server's bytes in chunks of that size, with
/// `golden` as the expected server name. An empty input does nothing.
///
/// # Panics
///
/// Panics when the chunk size changes what the attempt writes or how it
/// ends, when a result changes or more bytes are written after it is
/// known, when the attempt writes anything but CONNECT and PING, a PONG
/// per PING, and the JetStream request once and only when asked, when it
/// is ready without that whole exchange, or when a failure has a reason
/// the exchange never gives or a detail over 256 characters or with a
/// control character.
pub fn nats_readiness_protocol(data: &[u8]) {
    let Some((&mode, server)) = data.split_first() else {
        return;
    };
    let expectation = Expectation {
        server_name: "golden".to_owned(),
        jetstream: mode & 1 == 1,
    };
    let chunk = usize::from(mode >> 1) + 1;
    let (written, result, exchange) = nats_attempt(&expectation, server, chunk);
    let (whole_written, whole_result, _) = nats_attempt(&expectation, server, usize::MAX);
    assert_eq!(
        written, whole_written,
        "the chunk size changed what the attempt wrote"
    );
    assert_eq!(
        result, whole_result,
        "the chunk size changed how the attempt ended"
    );
    check_nats_writes(&expectation, &exchange, &written, result.as_ref());
    if let Some(Err(failure)) = &result {
        let reasons = [
            nats::PROTOCOL_ERROR,
            nats::IDENTITY_MISMATCH,
            nats::TLS_REQUIRED,
            nats::AUTH_REQUIRED,
            nats::LAME_DUCK,
            nats::JETSTREAM_DISABLED,
            nats::JETSTREAM_UNEXPECTED,
            nats::JETSTREAM_UNAVAILABLE,
        ];
        assert!(
            reasons.contains(&failure.reason),
            "the exchange gave reason {}",
            failure.reason.as_str()
        );
        check_nats_detail(&failure.detail);
    }
    for failure in [
        exchange.closed(),
        exchange.reset(),
        exchange.timed_out(Duration::from_millis(2000)),
    ] {
        check_nats_detail(&failure.detail);
    }
}

fn package_files(files: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
    files
        .iter()
        .map(|(path, text)| ((*path).to_owned(), text.as_bytes().to_vec()))
        .collect()
}

/// The fake package, validated once.
fn fake_package() -> &'static ValidatedPackage {
    static PACKAGE: OnceLock<ValidatedPackage> = OnceLock::new();
    PACKAGE.get_or_init(|| {
        ValidatedPackage::from_files(
            PackageFiles::new("fake", package_files(&FAKE)),
            NameCheck::FolderName,
        )
        .expect("the embedded fake package validates")
    })
}

/// Validates a package and, when it passes, exercises its plans.
fn check_package(files: PackageFiles) {
    match ValidatedPackage::from_files(files, NameCheck::FolderName) {
        Ok(package) => exercise(&package),
        Err(err) => {
            assert!(!err.issues.is_empty(), "a rejected package has an issue");
            assert!(err.issues.len() <= MAX_ISSUES + 1, "issues are capped");
            if err.issues.len() == MAX_ISSUES + 1 {
                assert_eq!(err.issues[MAX_ISSUES].code, IssueCode::TooManyIssues);
            }
        }
    }
}

/// The invariants of a valid package: it validates again from its own
/// files with the same key, and every plan built from its sample contexts
/// keeps programs in the runtime, paths in the instance, arguments within
/// their limit, the closed env, and rendered output that parses.
fn exercise(package: &ValidatedPackage) {
    let again = ValidatedPackage::from_files(package.files().clone(), NameCheck::Skip)
        .expect("a validated package validates again from its own files");
    assert_eq!(
        again.key(),
        package.key(),
        "the digest changed on a re-read"
    );
    for with_items in [false, true] {
        let Some(context) = sample_context(&package.manifest, &package.settings, with_items) else {
            continue;
        };
        if let Ok(plan) = package.init_plan(&context) {
            check_steps(package, &context, &plan.steps);
            for path in &plan.cleanup {
                assert_inside(&context, path);
            }
        }
        if let Ok(plan) = package.run_plan(&context) {
            check_steps(package, &context, &plan.render);
            check_command(&context, &plan.spawn);
            for probe in [&plan.readiness.probe, &plan.health.probe] {
                if let ProbeSpec::Exec(run) = probe {
                    check_secretless_command(&context, &run.command);
                }
            }
            if let StopPlan::Command { run, .. } = &plan.stop {
                check_secretless_command(&context, &run.command);
            }
            for path in &plan.log_files {
                assert_inside(&context, path);
            }
        }
        if let Ok(plan) = package.reload_plan(&context) {
            check_steps(package, &context, &plan.steps);
            if let ReloadAction::Run(run) = &plan.action {
                check_secretless_command(&context, &run.command);
            }
        }
    }
}

/// Every secret field of a package, resolved to [`AWKWARD_SECRET`].
fn awkward_secrets(package: &ValidatedPackage) -> ResolvedSecrets {
    ResolvedSecrets::new(
        package
            .settings
            .fields()
            .filter(|field| field.is_secret())
            .map(|field| {
                let value = SecretValue::new(AWKWARD_SECRET.to_owned())
                    .expect("the awkward secret is UTF-8 without NUL");
                (field.name().clone(), value)
            })
            .collect(),
    )
}

fn check_steps(package: &ValidatedPackage, context: &PlanContext, steps: &[InitStep]) {
    let secrets = awkward_secrets(package);
    for step in steps {
        match step {
            InitStep::EnsureDirectories(dirs) => {
                for dir in dirs {
                    assert_inside(context, dir);
                }
            }
            InitStep::WriteFile(write) => {
                assert_inside(context, &write.path);
                let format = package
                    .manifest
                    .config_files
                    .get(&write.content.config_file)
                    .map(|spec| spec.format)
                    .expect("a written file belongs to a config file");
                match write.content.materialize(&secrets) {
                    Ok(bytes) => {
                        let text = String::from_utf8(bytes).expect("rendered output is UTF-8");
                        assert!(
                            format.output_parses(&text),
                            "{} renders {format:?} that does not parse",
                            write.content.config_file
                        );
                    }
                    Err(err) => assert!(
                        matches!(
                            err,
                            PlanError::UnrepresentableValue { .. }
                                | PlanError::OutputTooLarge { .. }
                        ),
                        "materialize failed with {err:?}"
                    ),
                }
            }
            InitStep::MaterializeSecret { file_name, .. } => assert!(
                is_component(file_name),
                "a materialized secret's file name is not one component"
            ),
            InitStep::Run(run) => check_command(context, &run.command),
        }
    }
}

fn check_command(context: &PlanContext, command: &Command) {
    assert_eq!(
        command.program.runtime_root, context.runtime.root,
        "a program outside the runtime"
    );
    assert!(
        command
            .program
            .relative
            .components()
            .all(|component| matches!(component, Component::Normal(_))),
        "a program path that is not plain components"
    );
    assert!(
        command.args.iter().all(|arg| arg.len() <= ARGUMENT_MAX_LEN),
        "an argument above the limit"
    );
    assert_inside(context, &command.cwd);
    let keys: Vec<&str> = command.env.iter().map(|(key, _)| key.as_str()).collect();
    assert!(
        keys.windows(2).all(|pair| pair[0] < pair[1]),
        "env keys are sorted and unique"
    );
    for base in ["HOME", "PATH", "TMPDIR"] {
        assert!(keys.contains(&base), "env lacks {base}");
    }
    assert!(
        command.env.iter().any(|(key, value)| key == "PATH"
            && matches!(value, EnvValue::Text(text) if text == CHILD_PATH)),
        "PATH is not the base PATH"
    );
}

/// Checks a command that never resolves secrets: an exec probe, a stop
/// command, or a reload run.
fn check_secretless_command(context: &PlanContext, command: &Command) {
    check_command(context, command);
    assert!(
        command
            .env
            .iter()
            .all(|(_, value)| matches!(value, EnvValue::Text(_))),
        "a command that resolves no secret has a secret env value"
    );
}

/// Asserts that a plan path is under one of the instance folders.
fn assert_inside(context: &PlanContext, path: &Path) {
    let inside = InstanceDir::ALL
        .into_iter()
        .any(|dir| path.starts_with(dir.path(&context.paths)));
    assert!(inside, "{} is outside the instance", path.display());
    assert!(
        path.components()
            .all(|component| !matches!(component, Component::ParentDir | Component::CurDir)),
        "{} holds . or ..",
        path.display()
    );
}

/// Parses settings schema text in the dialect, as the validator does.
fn parse_settings(text: &str, issues: &mut Issues) -> Option<SettingsSchema> {
    let file = "settings.schema.json";
    let value = parse_json(file, text)
        .map_err(|issue| issues.push(issue))
        .ok()?;
    let raw = decode_json::<RawSettingsSchema>(file, value, IssueCode::SettingsSchemaKeyword)
        .map_err(|issue| issues.push(issue))
        .ok()?;
    parse_schema(raw, &mut FileIssues::new(file, issues))
}

/// The fixed documents the settings harness validates: empty, a secret
/// reference, the schema's sample values with and without items, and every
/// field set to an awkward value.
fn documents(schema: &SettingsSchema) -> Vec<serde_json::Value> {
    let mut awkward = serde_json::Map::new();
    let mut texts = serde_json::Map::new();
    for (index, field) in schema.fields().enumerate() {
        let value = match index % 4 {
            0 => serde_json::json!("-x\u{1}"),
            1 => serde_json::json!(5.0),
            2 => serde_json::json!([{}, { "name": "a" }]),
            _ => serde_json::json!(-9_007_199_254_740_993_i64),
        };
        awkward.insert(field.name().to_string(), value);
        texts.insert(
            field.name().to_string(),
            serde_json::json!(SAMPLE_SECRET_REF),
        );
    }
    vec![
        serde_json::json!({}),
        serde_json::json!({ "authToken": SAMPLE_SECRET_REF }),
        schema.sample_values(false).to_json(),
        schema.sample_values(true).to_json(),
        serde_json::Value::Object(awkward),
        serde_json::Value::Object(texts),
    ]
}

/// What one `nats-readiness` attempt wrote, how it ended, and the
/// exchange, after feeding `server` in chunks of `chunk` bytes until a
/// result.
fn nats_attempt(
    expectation: &Expectation,
    server: &[u8],
    chunk: usize,
) -> (Vec<u8>, Option<Result<(), NativeProbeFailure>>, Exchange) {
    let mut exchange = Exchange::new(expectation.clone(), [0x5a; 16]);
    let mut written = Vec::new();
    for piece in server.chunks(chunk) {
        let step = exchange.feed(piece);
        written.extend_from_slice(&step.write);
        if let Some(done) = step.done {
            let again = exchange.feed(b"PING\r\n");
            assert!(again.write.is_empty(), "the attempt wrote after its result");
            assert_eq!(
                again.done.as_ref(),
                Some(&done),
                "a result changed after it was known"
            );
            return (written, Some(done), exchange);
        }
    }
    let result = exchange.result().cloned();
    (written, result, exchange)
}

/// Checks that `written` is CONNECT and PING, then PONGs and at most one
/// JetStream request (only when asked), and that a ready attempt sent the
/// whole exchange.
fn check_nats_writes(
    expectation: &Expectation,
    exchange: &Exchange,
    written: &[u8],
    result: Option<&Result<(), NativeProbeFailure>>,
) {
    let connect = connect_bytes();
    let request = exchange.jetstream_request();
    let Some(mut rest) = written.strip_prefix(connect.as_slice()) else {
        assert!(written.is_empty(), "the attempt wrote before CONNECT");
        assert!(result != Some(&Ok(())), "ready without CONNECT");
        return;
    };
    let mut requested = false;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(b"PONG\r\n".as_slice()) {
            rest = after;
        } else if let Some(after) = rest
            .strip_prefix(request.as_slice())
            .filter(|_| expectation.jetstream && !requested)
        {
            rest = after;
            requested = true;
        } else {
            panic!("the attempt wrote bytes outside the protocol");
        }
    }
    if result == Some(&Ok(())) {
        assert_eq!(
            requested, expectation.jetstream,
            "ready without the JetStream request it needs"
        );
    }
}

/// A detail from the exchange fits the observation limit before the
/// runner of spec 0010 redacts and cuts it, and has no control character.
fn check_nats_detail(detail: &str) {
    assert!(
        detail.chars().count() <= 256,
        "a detail has more than 256 characters"
    );
    assert!(
        !detail.chars().any(char::is_control),
        "a detail has a control character"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_packages_pass_their_own_harness() {
        manifest(FAKE[0].1.as_bytes());
        let mut template_input = vec![0];
        template_input.extend_from_slice(FAKE[3].1.as_bytes());
        template(&template_input);
        let mut routes = vec![3];
        routes.extend_from_slice(TRAEFIK[4].1.as_bytes());
        template(&routes);
        settings_schema(FAKE[2].1.as_bytes());
        interpolation(b"--data=${instance.data}");
    }

    #[test]
    fn the_fake_package_validates_and_its_plans_hold_the_invariants() {
        let package = fake_package();
        let context = sample_context(&package.manifest, &package.settings, true).unwrap();

        assert!(
            package.init_plan(&context).is_ok(),
            "the sample context plans"
        );
        assert!(
            package.run_plan(&context).is_ok(),
            "the sample context plans"
        );
        assert!(
            package.reload_plan(&context).is_ok(),
            "the sample context plans"
        );
        exercise(package);
    }

    #[test]
    fn broken_and_odd_inputs_do_not_panic() {
        for input in [
            &b""[..],
            b"\xff\xfe",
            b"apiVersion: stackly/v1alpha1\nkind: Provider\n",
            b"${",
            b"$${instance.data}/../x",
            b"${instance.data}/../config",
            b"${runtime.executable} -c",
            b".s.PGSQL.${endpoint.http.port}xy",
            b"{{#each settings.greeting as g}}{{/each}}",
        ] {
            manifest(input);
            template(input);
            interpolation(input);
            settings_schema(input);
            nats_readiness_protocol(input);
        }
    }
}
