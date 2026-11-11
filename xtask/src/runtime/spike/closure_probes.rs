//! The closure probes of the spike (spec 0003 AC-7, *Spike dari paket
//! provider*): what the running server proves about the libraries inside
//! the artifact, picked by provider name. This is knowledge about the
//! artifact, not about how the provider runs, so it stays in xtask while
//! the lifecycle comes from the provider package (spec 0036 AC-16).
//!
//! The probes talk to the server through a [`ProbeClient`], so unit tests
//! can check every command and every verdict without a server.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::process::{self, Finished};
use super::result::{Probe, StepStatus};

/// The trial database of the `icu` probe; the spike root that holds the
/// cluster is removed at the end of the run, so it is never dropped.
pub const ICU_PROBE_DATABASE: &str = "stackly_icu_probe";
/// The PostgreSQL superuser of the package: `postgres`, a literal in its
/// `initdb` arguments, so the OS user name never reaches the evidence.
const POSTGRES_SUPERUSER: &str = "postgres";
/// The settings field that holds the PostgreSQL superuser password.
const POSTGRES_PASSWORD_FIELD: &str = "superuserPassword";

/// What the probes know about the running instance, from the context the
/// package plans were built in.
#[derive(Debug, Clone, Default)]
pub struct SpikeFacts {
    /// Every endpoint by name.
    pub endpoints: BTreeMap<String, EndpointFact>,
    /// The validated settings, as JSON.
    #[expect(
        dead_code,
        reason = "spec 0036 hands the settings to the probes; no probe reads one yet"
    )]
    pub settings: serde_json::Value,
    /// The fixture value of every secret field, by field name.
    pub secrets: BTreeMap<String, String>,
}

/// The address of one endpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndpointFact {
    /// The port of a TCP or UDP endpoint.
    pub port: Option<u16>,
    /// The folder of a Unix endpoint's socket.
    pub socket_dir: Option<PathBuf>,
}

impl SpikeFacts {
    fn port(&self, endpoint: &str) -> Result<u16> {
        self.endpoints
            .get(endpoint)
            .and_then(|fact| fact.port)
            .with_context(|| format!("the package has no port for endpoint {endpoint}"))
    }

    fn socket_dir(&self, endpoint: &str) -> Result<&Path> {
        self.endpoints
            .get(endpoint)
            .and_then(|fact| fact.socket_dir.as_deref())
            .with_context(|| format!("the package has no socket folder for endpoint {endpoint}"))
    }

    fn secret(&self, field: &str) -> Result<&str> {
        self.secrets
            .get(field)
            .map(String::as_str)
            .with_context(|| format!("the package has no secret field {field}"))
    }
}

/// How a probe reaches the server and the artifact.
pub trait ProbeClient {
    /// Runs the inventory executable called `name` with `args` and the
    /// extra `env`, to completion.
    fn run(&self, name: &str, args: &[String], env: &[(String, String)]) -> Result<Finished>;
    /// One HTTP GET on a loopback port: the status and the body.
    fn http_get(&self, port: u16, path: &str) -> Result<(u16, String)>;
    /// The size of a file, when it exists.
    fn file_len(&self, path: &Path) -> Option<u64>;
    /// The run folder, where a probe may write.
    fn run_dir(&self) -> &Path;
}

/// The closure probes of `provider`, run in order; each runs whatever the
/// others gave.
pub fn probes(provider: &str, facts: &SpikeFacts, client: &dyn ProbeClient) -> Vec<Probe> {
    match provider {
        "postgresql" => Postgres { facts, client }.probes(),
        "nats" => vec![nats_jetstream(facts, client)],
        _ => Vec::new(),
    }
}

fn probe(name: &str, outcome: Result<String>) -> Probe {
    match outcome {
        Ok(detail) => Probe {
            name: name.to_owned(),
            status: StepStatus::Pass,
            detail,
        },
        Err(err) => Probe {
            name: name.to_owned(),
            status: StepStatus::Fail,
            detail: format!("{err:#}"),
        },
    }
}

/// JetStream answers on the monitoring endpoint.
fn nats_jetstream(facts: &SpikeFacts, client: &dyn ProbeClient) -> Probe {
    let path = "/healthz?js-enabled-only=true";
    probe(
        "jetstream",
        facts
            .port("monitoring")
            .and_then(|port| client.http_get(port, path))
            .and_then(|(code, body)| {
                if code == 200 {
                    Ok(format!("GET {path} gave 200 {}", body.trim()))
                } else {
                    bail!("GET {path} gave {code} {}", body.trim())
                }
            }),
    )
}

/// The four AC-7 probes, over the package's private socket as its
/// superuser, with the fixture password in `PGPASSWORD`.
struct Postgres<'a> {
    facts: &'a SpikeFacts,
    client: &'a dyn ProbeClient,
}

impl Postgres<'_> {
    fn probes(&self) -> Vec<Probe> {
        vec![
            probe("icu", self.icu()),
            probe(
                "pgcrypto",
                self.expect(
                    &[
                        "create extension pgcrypto",
                        "select length(gen_random_bytes(16))",
                    ],
                    "16",
                ),
            ),
            probe(
                "lz4",
                self.expect(
                    &[
                        "create table stackly_lz4 (value text compression lz4)",
                        "insert into stackly_lz4 values (repeat('a', 10000))",
                        "select pg_column_compression(value) from stackly_lz4",
                    ],
                    "lz4",
                ),
            ),
            probe("zstd", self.zstd()),
        ]
    }

    /// `-h <socket folder> -p <client port> -U postgres`.
    fn connection(&self) -> Result<Vec<String>> {
        Ok(vec![
            "-h".to_owned(),
            self.facts.socket_dir("socket")?.display().to_string(),
            "-p".to_owned(),
            self.facts.port("client")?.to_string(),
            "-U".to_owned(),
            POSTGRES_SUPERUSER.to_owned(),
        ])
    }

    fn env(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![(
            "PGPASSWORD".to_owned(),
            self.facts.secret(POSTGRES_PASSWORD_FIELD)?.to_owned(),
        )])
    }

    /// `psql -X -w -v ON_ERROR_STOP=1 -At <connection> -d postgres` with
    /// one `-c` per statement; the last line it prints.
    fn psql(&self, statements: &[&str]) -> Result<String> {
        let mut args: Vec<String> = ["-X", "-w", "-v", "ON_ERROR_STOP=1", "-At"]
            .map(str::to_owned)
            .to_vec();
        args.extend(self.connection()?);
        args.extend(["-d".to_owned(), "postgres".to_owned()]);
        for statement in statements {
            args.extend(["-c".to_owned(), (*statement).to_owned()]);
        }
        let finished = self.client.run("psql", &args, &self.env()?)?;
        let stdout = success(&finished, &statements.join("; "))?;
        Ok(stdout
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .unwrap_or_default()
            .to_owned())
    }

    fn expect(&self, statements: &[&str], expected: &str) -> Result<String> {
        let found = self.psql(statements)?;
        let what = statements.join("; ");
        if found == expected {
            Ok(format!("{what} gave {found}"))
        } else {
            bail!("{what} gave `{found}`, not `{expected}`")
        }
    }

    /// A trial database with the ICU locale provider, made from
    /// `template0`, whose `datlocprovider` must read `i`. The two statements
    /// go in two calls, since `CREATE DATABASE` cannot run inside the
    /// transaction one `-c` with both would be.
    fn icu(&self) -> Result<String> {
        let create = format!(
            "create database {ICU_PROBE_DATABASE} template template0 locale_provider icu icu_locale 'und'"
        );
        let select = format!(
            "select datlocprovider from pg_database where datname = '{ICU_PROBE_DATABASE}'"
        );
        self.psql(&[&create])?;
        let provider = self.psql(&[&select])?;
        if provider != "i" {
            bail!("{select} gave `{provider}`, not `i`");
        }
        let info = format!(
            "select datlocale || ', collversion ' || coalesce(datcollversion, 'none') from pg_database where datname = '{ICU_PROBE_DATABASE}'"
        );
        Ok(match self.psql(&[&info]) {
            Ok(locale) => format!("{ICU_PROBE_DATABASE}: provider i, locale {locale}"),
            Err(_) => format!("{ICU_PROBE_DATABASE}: provider i"),
        })
    }

    /// `pg_dump --compress=zstd` of `postgres` into a file in the run
    /// folder, which must not be empty.
    fn zstd(&self) -> Result<String> {
        let dump = self.client.run_dir().join("dump.zst");
        let mut args = vec!["-w".to_owned()];
        args.extend(self.connection()?);
        args.extend([
            "--compress=zstd".to_owned(),
            "-f".to_owned(),
            dump.display().to_string(),
            "postgres".to_owned(),
        ]);
        let finished = self.client.run("pg_dump", &args, &self.env()?)?;
        success(&finished, "pg_dump --compress=zstd")?;
        match self.client.file_len(&dump) {
            Some(0) => bail!("pg_dump --compress=zstd wrote an empty file"),
            Some(size) => Ok(format!("pg_dump --compress=zstd wrote {size} bytes")),
            None => bail!("pg_dump wrote no {}", dump.display()),
        }
    }
}

/// The stdout of a command that exited 0, or an error with its stderr.
fn success(finished: &Finished, what: &str) -> Result<String> {
    match finished.status {
        Some(status) if status.success() => Ok(finished.stdout.clone()),
        Some(status) => bail!(
            "{what} failed with {}: {}",
            process::describe(status),
            finished.complaint()
        ),
        None => bail!("{what} did not finish in time"),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use super::*;

    /// One call a script saw: the executable, its args, and its env.
    type Call = (String, Vec<String>, Vec<(String, String)>);

    /// One scripted answer per call, in order, and the calls it saw.
    #[derive(Default)]
    struct Script {
        answers: RefCell<Vec<(i32, String, String)>>,
        calls: RefCell<Vec<Call>>,
        http: Option<(u16, String)>,
        file_len: Option<u64>,
    }

    impl Script {
        fn answering(answers: &[(i32, &str, &str)]) -> Self {
            Self {
                answers: RefCell::new(
                    answers
                        .iter()
                        .rev()
                        .map(|(code, out, err)| (*code, (*out).to_owned(), (*err).to_owned()))
                        .collect(),
                ),
                file_len: Some(4096),
                ..Self::default()
            }
        }

        fn sql(&self) -> Vec<Vec<String>> {
            self.calls
                .borrow()
                .iter()
                .map(|(_, args, _)| {
                    args.windows(2)
                        .filter(|pair| pair[0] == "-c")
                        .map(|pair| pair[1].clone())
                        .collect()
                })
                .collect()
        }
    }

    impl ProbeClient for Script {
        fn run(&self, name: &str, args: &[String], env: &[(String, String)]) -> Result<Finished> {
            self.calls
                .borrow_mut()
                .push((name.to_owned(), args.to_vec(), env.to_vec()));
            let (code, stdout, stderr) =
                self.answers
                    .borrow_mut()
                    .pop()
                    .unwrap_or((0, String::new(), String::new()));
            Ok(Finished {
                status: Some(ExitStatus::from_raw(code << 8)),
                stdout,
                stderr,
            })
        }

        fn http_get(&self, _port: u16, _path: &str) -> Result<(u16, String)> {
            self.http.clone().context("no server")
        }

        fn file_len(&self, _path: &Path) -> Option<u64> {
            self.file_len
        }

        fn run_dir(&self) -> &Path {
            Path::new("/spike/run")
        }
    }

    fn postgres_facts() -> SpikeFacts {
        let mut facts = SpikeFacts::default();
        facts.endpoints.insert(
            "client".to_owned(),
            EndpointFact {
                port: Some(40123),
                socket_dir: None,
            },
        );
        facts.endpoints.insert(
            "socket".to_owned(),
            EndpointFact {
                port: None,
                socket_dir: Some(PathBuf::from("/tmp/sk-0011aabb/i/abc")),
            },
        );
        facts.secrets.insert(
            "superuserPassword".to_owned(),
            "stackly-fixture-credential-pw".to_owned(),
        );
        facts
    }

    #[test]
    fn postgresql_runs_four_probes_in_a_fixed_order_as_the_package_superuser() {
        let script = Script::answering(&[
            (0, "CREATE DATABASE\n", ""),
            (0, "i\n", ""),
            (0, "und, collversion 153.120\n", ""),
            (0, "CREATE EXTENSION\n16\n", ""),
            (0, "CREATE TABLE\nINSERT 0 1\nlz4\n", ""),
            (0, "", ""),
        ]);

        let probes = probes("postgresql", &postgres_facts(), &script);

        let names: Vec<&str> = probes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["icu", "pgcrypto", "lz4", "zstd"]);
        assert!(
            probes.iter().all(|p| p.status == StepStatus::Pass),
            "{probes:#?}"
        );
        assert_eq!(
            probes[0].detail,
            "stackly_icu_probe: provider i, locale und, collversion 153.120"
        );
        let calls = script.calls.borrow();
        for (name, args, env) in calls.iter() {
            assert!(name == "psql" || name == "pg_dump", "{name}");
            assert_eq!(
                env,
                &[(
                    "PGPASSWORD".to_owned(),
                    "stackly-fixture-credential-pw".to_owned()
                )]
            );
            let joined = args.join(" ");
            assert!(
                joined.contains("-h /tmp/sk-0011aabb/i/abc -p 40123 -U postgres"),
                "{joined}"
            );
            assert!(args.contains(&"-w".to_owned()), "{joined}");
            assert!(!joined.contains("stackly_readiness"), "{joined}");
        }
        let psql = &calls[0].1;
        assert_eq!(&psql[..5], ["-X", "-w", "-v", "ON_ERROR_STOP=1", "-At"]);
        let zstd = &calls[5];
        assert_eq!(zstd.0, "pg_dump");
        assert!(zstd.1.contains(&"--compress=zstd".to_owned()));
        assert!(zstd.1.contains(&"/spike/run/dump.zst".to_owned()));
    }

    #[test]
    fn the_icu_probe_sends_its_two_statements_in_two_calls() {
        let script = Script::answering(&[
            (0, "", ""),
            (0, "i\n", ""),
            (0, "und, collversion none", ""),
        ]);

        let probe = Postgres {
            facts: &postgres_facts(),
            client: &script,
        }
        .icu();

        assert!(probe.is_ok(), "{probe:?}");
        let sql = script.sql();
        assert_eq!(
            sql[0],
            [
                "create database stackly_icu_probe template template0 locale_provider icu icu_locale 'und'"
            ]
        );
        assert_eq!(
            sql[1],
            ["select datlocprovider from pg_database where datname = 'stackly_icu_probe'"]
        );
    }

    #[test]
    fn the_icu_probe_fails_on_another_provider_or_a_refused_create() {
        let builtin = Script::answering(&[(0, "", ""), (0, "b\n", "")]);
        let refused = Script::answering(&[(1, "", "ERROR:  ICU is not supported in this build")]);
        let facts = postgres_facts();

        let builtin = probes("postgresql", &facts, &builtin);
        let refused_probes = probes("postgresql", &facts, &refused);

        assert_eq!(builtin[0].status, StepStatus::Fail);
        assert!(
            builtin[0].detail.contains("gave `b`, not `i`"),
            "{}",
            builtin[0].detail
        );
        assert_eq!(refused_probes[0].status, StepStatus::Fail);
        assert!(
            refused_probes[0]
                .detail
                .contains("create database stackly_icu_probe")
                && refused_probes[0].detail.contains("ICU is not supported"),
            "{}",
            refused_probes[0].detail
        );
        assert_eq!(
            refused_probes.len(),
            4,
            "every probe runs whatever icu gave"
        );
    }

    #[test]
    fn an_empty_or_missing_dump_fails_the_zstd_probe() {
        let mut empty = Script::answering(&[]);
        empty.file_len = Some(0);
        let mut missing = Script::answering(&[]);
        missing.file_len = None;
        let facts = postgres_facts();

        let empty = Postgres {
            facts: &facts,
            client: &empty,
        }
        .zstd();
        let missing = Postgres {
            facts: &facts,
            client: &missing,
        }
        .zstd();

        assert!(empty.unwrap_err().to_string().contains("empty file"));
        assert!(missing.unwrap_err().to_string().contains("wrote no"));
    }

    #[test]
    fn nats_checks_jetstream_on_the_monitoring_port_and_others_have_no_probes() {
        let mut facts = SpikeFacts::default();
        facts.endpoints.insert(
            "monitoring".to_owned(),
            EndpointFact {
                port: Some(40200),
                socket_dir: None,
            },
        );
        let up = Script {
            http: Some((200, "{\"status\":\"ok\"}".to_owned())),
            ..Script::default()
        };
        let down = Script {
            http: Some((503, "jetstream unavailable".to_owned())),
            ..Script::default()
        };

        let up = probes("nats", &facts, &up);
        let down = probes("nats", &facts, &down);

        assert_eq!(up.len(), 1);
        assert_eq!(up[0].name, "jetstream");
        assert_eq!(up[0].status, StepStatus::Pass);
        assert_eq!(down[0].status, StepStatus::Fail);
        assert!(probes("traefik", &facts, &Script::default()).is_empty());
        assert!(probes("valkey", &facts, &Script::default()).is_empty());
    }
}
