//! Seeds a development root with the fake runtime and the fake provider
//! package, so `stackly` and the app can run a fake instance against a real
//! `stacklyd` without the runtime install of feature 18 (spec 0014).
//!
//! ```text
//! skeleton_seed --root <path> [--fake-server <path>] [--json]
//! ```
//!
//! The root goes through the same checks as `STACKLY_ROOT` (spec 0004), the
//! seed holds the daemon lock of the root while it writes, and it never
//! touches the default root. Run it again on the same root and it writes
//! nothing and prints the same IDs. Exit code 0 on success, 1 for a refusal
//! or an error, 2 for bad arguments.

#[cfg(target_os = "macos")]
mod seed {
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;

    use clap::Parser;
    use serde_json::json;
    use stackly_core::supervisor::fixture::{FAKE_EXECUTABLE, FixtureError, seed_fake};
    use stackly_core::{daemon_info, prepare_layout};
    use stackly_platform_macos::MacosPlatform;
    use stackly_protocol::Timestamp;
    use stackly_storage::{RegistryOpen, open_registry};
    use stacklyd::lock::{DaemonLock, LockError, LockOwner};
    use uuid::Uuid;

    /// Seeds a development root with the fake runtime and the fake provider.
    #[derive(Debug, Parser)]
    #[command(about)]
    struct Args {
        /// The root to seed, as `STACKLY_ROOT` would name it.
        #[arg(long)]
        root: PathBuf,
        /// The `stackly-fake-server` binary; by default the one in the
        /// profile folder above this example.
        #[arg(long)]
        fake_server: Option<PathBuf>,
        /// Print one JSON object instead of text.
        #[arg(long)]
        json: bool,
    }

    pub(super) fn main() -> ExitCode {
        let args = Args::parse();
        let json = args.json;
        match run(args) {
            Ok(report) => {
                if json {
                    println!("{}", report.to_json());
                } else {
                    print!("{}", report.to_text());
                }
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::from(1)
            }
        }
    }

    struct Report {
        root: String,
        runtime_id: String,
        provider: String,
        revision: String,
        create_command: String,
    }

    impl Report {
        fn to_text(&self) -> String {
            format!(
                "root      {}\nruntime   {}\nprovider  {} {}\nnext      {}\n",
                self.root, self.runtime_id, self.provider, self.revision, self.create_command,
            )
        }

        fn to_json(&self) -> String {
            json!({
                "root": self.root,
                "runtimeId": self.runtime_id,
                "provider": { "name": self.provider, "revision": self.revision },
                "createCommand": self.create_command,
            })
            .to_string()
        }
    }

    fn run(args: Args) -> Result<Report, String> {
        let fake_server = match args.fake_server {
            Some(path) => path,
            None => default_fake_server()?,
        };
        // The same order as `stacklyd::startup`: the root checks, the
        // layout, the lock, then the registry.
        let platform = MacosPlatform::new(Some(args.root.clone().into_os_string()))
            .map_err(|error| error.to_string())?;
        let root = args
            .root
            .canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", args.root.display()))?
            .display()
            .to_string();
        let prepared = prepare_layout(&platform).map_err(|error| error.to_string())?;
        let layout = prepared.layout();
        let owner = LockOwner::new(
            std::process::id(),
            Uuid::now_v7(),
            Timestamp::from_jiff(platform.clock().now()),
            daemon_info().version,
        );
        let lock = DaemonLock::acquire(&layout.daemon_lock(), owner).map_err(|error| {
            match error {
                LockError::Held { .. } => {
                    "a stacklyd holds the daemon lock of this root; stop the daemon for this root first"
                        .to_owned()
                }
                other => other.to_string(),
            }
        })?;
        let registry = match open_registry(&platform, layout, daemon_info().version)
            .map_err(|error| error.to_string())?
        {
            RegistryOpen::Ready { registry, .. } => registry,
            RegistryOpen::Recovery(_) => {
                return Err("the registry of this root is in recovery mode".to_owned());
            }
        };
        if !fake_server.is_file() {
            return Err(format!(
                "{} is missing; build it with: cargo build -p stackly-fake-server",
                fake_server.display(),
            ));
        }
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
        let seeded =
            seed_fake(&platform, layout, &registry, &fake_server, &package).map_err(describe)?;
        // The registry closes first (it waits for its writer thread), then
        // the lock goes.
        drop(registry);
        if let Err(error) = lock.release() {
            eprintln!("warning: cannot release the daemon lock: {error}");
        }

        let runtime_id = seeded.runtime_id.as_uuid().to_string();
        let provider = seeded.provider.provider().to_string();
        let revision = seeded.provider.revision().as_str().to_owned();
        let create_command = format!(
            "STACKLY_ROOT={} stackly instance create demo --runtime {} --provider {} --revision {} --wait",
            quote(&root),
            quote(&runtime_id),
            quote(&provider),
            quote(&revision),
        );
        Ok(Report {
            root,
            runtime_id,
            provider,
            revision,
            create_command,
        })
    }

    /// `stackly-fake-server` in the profile folder above this example
    /// (`target/debug/` for `target/debug/examples/skeleton_seed`).
    fn default_fake_server() -> Result<PathBuf, String> {
        let exe = std::env::current_exe()
            .map_err(|error| format!("cannot find this executable: {error}"))?;
        exe.parent()
            .and_then(Path::parent)
            .map(|profile| profile.join(FAKE_EXECUTABLE))
            .ok_or_else(|| "this executable has no profile folder above it".to_owned())
    }

    fn describe(error: FixtureError) -> String {
        match error {
            FixtureError::Conflict { what } => {
                format!("{what}; seed a new root instead")
            }
            FixtureError::Leftover { path } => format!(
                "{} exists without its registry row; seed a new root instead",
                path.display(),
            ),
            other => other.to_string(),
        }
    }

    /// Quotes `value` for a POSIX shell unless it only holds safe
    /// characters.
    fn quote(value: &str) -> String {
        let safe = !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._/:@%+=,-".contains(c));
        if safe {
            value.to_owned()
        } else {
            format!("'{}'", value.replace('\'', "'\\''"))
        }
    }
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    seed::main()
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    eprintln!("error: skeleton_seed runs on macOS only");
    std::process::ExitCode::from(1)
}
