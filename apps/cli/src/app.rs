//! The command line (spec 0008, *Struktur perintah dan flag global*): the
//! clap tree, the scan of the raw arguments for `--json` and `--color`
//! before clap parses them, the `cli.usage` envelope for refused arguments,
//! the environment, the terminal checks, and the dispatch.

use std::ffi::OsString;
use std::io::{BufReader, IsTerminal};

use anstream::AutoStream;
use clap::error::ErrorKind;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use jiff::tz::TimeZone;
use stackly_protocol::ProtocolVersion;

use crate::commands::completion::CompletionArgs;
use crate::commands::daemon::DaemonCommand;
use crate::commands::execute;
use crate::commands::instance::InstanceCommand;
use crate::commands::manpage::ManpageArgs;
use crate::commands::operation::OperationCommand;
use crate::commands::runtime::RuntimeCommand;
use crate::commands::secret::SecretCommand;
use crate::context::Context;
use crate::env;
use crate::exit::{EXIT_SUCCESS, EXIT_TABLE, EXIT_USAGE};
use crate::failure::CliFailure;
use crate::output::stderr::StderrSink;
use crate::output::{CommandOutput, Output, OutputMode};
use crate::panic;
use crate::progress::ProgressMode;
use crate::session::Connector;

/// The text after the root help: the environment and the exit codes.
const ROOT_AFTER_HELP: &str = "\
Environment:
  STACKLY_ROOT      run against the daemon of another root, as stacklyd does
  STACKLY_NO_INPUT  1 or true: never wait for input, like --no-input
  NO_COLOR          set: no color in auto mode
  CLICOLOR_FORCE    set and not 0: color even when not a terminal

";

/// The Stackly command line client. It only talks to stacklyd and never
/// starts it.
#[derive(Debug, Parser)]
#[command(
    name = "stackly",
    about = "The Stackly command line client: talks to stacklyd, never starts it",
    arg_required_else_help = true
)]
pub struct Cli {
    /// The flags every command takes.
    #[command(flatten)]
    pub global: GlobalArgs,
    /// The command.
    #[command(subcommand)]
    pub command: Command,
}

/// The flags every command takes, in any position.
#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Write one line of JSON on stdout instead of text
    #[arg(long, global = true)]
    pub json: bool,
    /// When to color text
    #[arg(long, global = true, value_enum, value_name = "WHEN", default_value_t = ColorWhen::Auto)]
    pub color: ColorWhen,
    /// Never wait for input; a prompt or confirmation fails instead
    #[arg(long, global = true)]
    pub no_input: bool,
    /// No progress, text warnings, or hints; errors still show
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// One line on stderr per daemon request
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

/// `--color`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    /// Color when the stream is a terminal, honoring `NO_COLOR`,
    /// `CLICOLOR`, `CLICOLOR_FORCE`, and `TERM=dumb`.
    Auto,
    /// Always color.
    Always,
    /// Never color.
    Never,
}

impl ColorWhen {
    fn anstream(self) -> anstream::ColorChoice {
        match self {
            Self::Auto => anstream::ColorChoice::Auto,
            Self::Always => anstream::ColorChoice::Always,
            Self::Never => anstream::ColorChoice::Never,
        }
    }

    fn clap(self) -> clap::ColorChoice {
        match self {
            Self::Auto => clap::ColorChoice::Auto,
            Self::Always => clap::ColorChoice::Always,
            Self::Never => clap::ColorChoice::Never,
        }
    }
}

/// The command groups. A new group adds one variant here and one module
/// under `commands/`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install, start, stop, or look at the Stackly daemon
    #[command(subcommand, arg_required_else_help = true)]
    Daemon(DaemonCommand),
    /// Create, start, stop, or show instances
    #[command(subcommand, arg_required_else_help = true)]
    Instance(InstanceCommand),
    /// List, inspect, cancel, resume, or abandon operations
    #[command(subcommand, arg_required_else_help = true)]
    Operation(OperationCommand),
    /// Install, list, or inspect managed runtimes
    #[command(subcommand, arg_required_else_help = true)]
    Runtime(RuntimeCommand),
    /// Store, inspect, reveal, or delete secrets in the Keychain
    #[command(subcommand, arg_required_else_help = true)]
    Secret(SecretCommand),
    /// Write the completion script of a shell
    #[command(after_help = "Examples:\n  stackly completion zsh\n  stackly completion bash --json")]
    Completion(CompletionArgs),
    /// Write the man pages
    #[command(
        hide = true,
        after_help = "Examples:\n  stackly manpage\n  stackly manpage --json"
    )]
    Manpage(ManpageArgs),
}

impl Command {
    /// Runs the command and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Daemon(command) => command.run(cx).await,
            Self::Instance(command) => command.run(cx).await,
            Self::Operation(command) => command.run(cx).await,
            Self::Runtime(command) => command.run(cx).await,
            Self::Secret(command) => command.run(cx).await,
            Self::Completion(args) => execute(args, cx).await,
            Self::Manpage(args) => execute(args, cx).await,
        }
    }
}

/// What the scan of the raw arguments found, up to the first `--`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreScan {
    /// An argument that is exactly `--json`.
    pub json: bool,
    /// The last valid `--color <v>` or `--color=<v>`.
    pub color: Option<ColorWhen>,
}

/// Scans `args` (without the program name) up to the first `--`.
#[must_use]
pub fn prescan(args: &[OsString]) -> PreScan {
    let mut scan = PreScan::default();
    let mut args = args.iter().map(|arg| arg.to_str());
    while let Some(arg) = args.next() {
        match arg {
            Some("--") => break,
            Some("--json") => scan.json = true,
            Some("--color") => {
                if let Some(Some(value)) = args.next()
                    && let Ok(color) = ColorWhen::from_str(value, false)
                {
                    scan.color = Some(color);
                }
            }
            Some(other) => {
                if let Some(value) = other.strip_prefix("--color=")
                    && let Ok(color) = ColorWhen::from_str(value, false)
                {
                    scan.color = Some(color);
                }
            }
            None => {}
        }
    }
    scan
}

/// `--version`: `<version> (protocol <major>.<minor>)`, after the name.
#[must_use]
pub fn version_text() -> String {
    format!(
        "{} (protocol {}.{})",
        env!("CARGO_PKG_VERSION"),
        ProtocolVersion::CURRENT.major,
        ProtocolVersion::CURRENT.minor
    )
}

/// The clap tree with the runtime version and the color of the scan.
#[must_use]
pub fn cli_command(color: Option<ColorWhen>) -> clap::Command {
    Cli::command()
        .version(version_text())
        .after_help(format!("{ROOT_AFTER_HELP}{EXIT_TABLE}"))
        .color(color.unwrap_or(ColorWhen::Auto).clap())
}

/// The snake_case `details.kind` of a clap error. clap's enum is non
/// exhaustive, so anything else is `other`.
#[must_use]
pub const fn error_kind(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidValue => "invalid_value",
        ErrorKind::UnknownArgument => "unknown_argument",
        ErrorKind::MissingRequiredArgument => "missing_required_argument",
        ErrorKind::InvalidSubcommand => "invalid_subcommand",
        ErrorKind::MissingSubcommand | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            "missing_subcommand"
        }
        ErrorKind::ArgumentConflict => "argument_conflict",
        ErrorKind::ValueValidation => "value_validation",
        ErrorKind::TooManyValues => "too_many_values",
        ErrorKind::TooFewValues => "too_few_values",
        ErrorKind::WrongNumberOfValues => "wrong_number_of_values",
        ErrorKind::NoEquals => "no_equals",
        ErrorKind::InvalidUtf8 => "invalid_utf8",
        _ => "other",
    }
}

/// The `cli.usage` message: the first line of clap's plain text without
/// `error: ` and a trailing period.
#[must_use]
pub fn usage_message(error: &clap::Error) -> String {
    let text = error.render().to_string();
    let first = text.lines().next().unwrap_or_default().trim();
    let first = first.strip_prefix("error: ").unwrap_or(first);
    first.strip_suffix('.').unwrap_or(first).to_owned()
}

/// The command path clap chose, such as `daemon status`.
#[must_use]
pub fn command_path(matches: &clap::ArgMatches) -> String {
    let mut path = Vec::new();
    let mut current = matches;
    while let Some((name, next)) = current.subcommand() {
        path.push(name);
        current = next;
    }
    path.join(" ")
}

/// Runs `stackly` with this process's arguments and returns the exit code.
pub fn run() -> u8 {
    let args: Vec<OsString> = std::env::args_os().collect();
    let scan = prescan(args.get(1..).unwrap_or_default());
    panic::set_json(scan.json);
    let matches = match cli_command(scan.color).try_get_matches_from(&args) {
        Ok(matches) => matches,
        Err(error) => return clap_failed(&error, scan.json),
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => return clap_failed(&error, scan.json),
    };
    let command = command_path(&matches);
    panic::set_command(&command, cli.global.verbose);
    let global = cli.global.clone();
    let mut output = real_output(&global, command);
    let environment = match env::read(env::executable()) {
        Ok(environment) => environment,
        Err(failure) => return output.emit(CommandOutput::<()>::failed(failure)),
    };
    output.socket = Some(environment.socket.clone());
    let stdin_tty = std::io::stdin().is_terminal();
    let stderr_tty = std::io::stderr().is_terminal();
    let interactive = stdin_tty && stderr_tty && !global.no_input && !environment.no_input;
    let progress = ProgressMode::choose(stderr_tty, environment.term_is_dumb, global.quiet);
    let connector = Connector {
        socket: environment.socket,
        uid: environment.uid,
        interactive,
    };
    let agent = environment.agent;
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return output.emit(CommandOutput::<()>::failed(
                CliFailure::runtime_unavailable(),
            ));
        }
    };
    let mut cx = Context::new(
        output,
        interactive,
        Box::new(BufReader::new(std::io::stdin())),
        progress,
        connector,
        agent,
    );
    let joined = runtime
        .block_on(async move { tokio::spawn(async move { cli.command.run(&mut cx).await }).await });
    // A macOS call that hangs cannot be cancelled and keeps its thread, so
    // the runtime is not waited for (spec 0015).
    runtime.shutdown_timeout(std::time::Duration::ZERO);
    panic::join(joined)
}

/// Handles an argument error, `--help`, or `--version` from clap.
fn clap_failed(error: &clap::Error, json: bool) -> u8 {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = error.print();
            EXIT_SUCCESS
        }
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand if !json => {
            let _ = error.print();
            EXIT_USAGE
        }
        kind if json => {
            let failure = CliFailure::usage(error_kind(kind), &usage_message(error));
            let global = GlobalArgs {
                json: true,
                color: ColorWhen::Never,
                no_input: true,
                quiet: true,
                verbose: false,
            };
            real_output(&global, String::new()).emit(CommandOutput::<()>::failed(failure))
        }
        _ => {
            let _ = error.print();
            EXIT_USAGE
        }
    }
}

/// The output on this process's stdout and stderr.
fn real_output(global: &GlobalArgs, command: String) -> Output {
    let choice = global.color.anstream();
    let resolved = |choice: anstream::ColorChoice| choice != anstream::ColorChoice::Never;
    let stdout_color = match choice {
        anstream::ColorChoice::Auto => resolved(AutoStream::choice(&std::io::stdout())),
        other => resolved(other),
    };
    let stderr_color = match choice {
        anstream::ColorChoice::Auto => resolved(AutoStream::choice(&std::io::stderr())),
        other => resolved(other),
    };
    Output {
        mode: if global.json {
            OutputMode::Json
        } else {
            OutputMode::Human
        },
        command,
        verbose: global.verbose,
        quiet: global.quiet,
        stdout: Box::new(std::io::stdout()),
        stdout_color,
        stderr: StderrSink::new(Box::new(std::io::stderr()), stderr_color),
        tz: TimeZone::system(),
        socket: None,
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_scan_finds_json_and_color_only_before_the_double_dash() {
        assert_eq!(
            prescan(&args(&["daemon", "status", "--json", "--color", "always"])),
            PreScan {
                json: true,
                color: Some(ColorWhen::Always)
            }
        );
        assert_eq!(
            prescan(&args(&[
                "--color=never",
                "x",
                "--",
                "--json",
                "--color=always"
            ])),
            PreScan {
                json: false,
                color: Some(ColorWhen::Never)
            }
        );
        assert_eq!(
            prescan(&args(&["--jsonx", "--color", "pink"])),
            PreScan::default()
        );
    }

    #[test]
    fn the_version_names_the_protocol() {
        assert_eq!(
            version_text(),
            format!(
                "{} (protocol {}.{})",
                env!("CARGO_PKG_VERSION"),
                ProtocolVersion::CURRENT.major,
                ProtocolVersion::CURRENT.minor
            )
        );
        cli_command(None).debug_assert();
    }

    fn parse_error(list: &[&str]) -> clap::Error {
        cli_command(Some(ColorWhen::Never))
            .try_get_matches_from(args(list))
            .unwrap_err()
    }

    #[test]
    fn clap_errors_map_to_their_snake_case_kind_and_a_plain_first_line() {
        let unknown = parse_error(&["stackly", "--nope"]);
        assert_eq!(error_kind(unknown.kind()), "unknown_argument");
        assert_eq!(
            usage_message(&unknown),
            "unexpected argument '--nope' found"
        );
        let subcommand = parse_error(&["stackly", "daemon", "nope"]);
        assert_eq!(error_kind(subcommand.kind()), "invalid_subcommand");
        let color = parse_error(&["stackly", "--color", "pink", "daemon", "status"]);
        assert_eq!(error_kind(color.kind()), "invalid_value");
        assert!(!usage_message(&color).contains('\u{1b}'));
        let empty = parse_error(&["stackly"]);
        assert_eq!(error_kind(empty.kind()), "missing_subcommand");
        assert_eq!(error_kind(ErrorKind::Io), "other");
    }

    #[test]
    fn the_command_path_joins_the_chosen_subcommands() {
        let matches = cli_command(None)
            .try_get_matches_from(args(&["stackly", "daemon", "status", "--json"]))
            .unwrap();
        assert_eq!(command_path(&matches), "daemon status");
    }

    #[test]
    fn global_flags_work_in_any_position() {
        let matches = cli_command(None)
            .try_get_matches_from(args(&["stackly", "-q", "daemon", "--json", "status", "-v"]))
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        assert!(cli.global.json && cli.global.quiet && cli.global.verbose);
    }

    #[test]
    fn the_root_help_lists_the_environment_and_every_exit_code() {
        let help = cli_command(Some(ColorWhen::Never))
            .render_long_help()
            .to_string();
        for needle in [
            "STACKLY_ROOT",
            "STACKLY_NO_INPUT",
            "NO_COLOR",
            "CLICOLOR_FORCE",
            "Exit codes:",
            "130",
            "--json",
            "--no-input",
        ] {
            assert!(help.contains(needle), "{needle} missing from:\n{help}");
        }
    }

    #[test]
    fn every_command_has_a_description_and_an_example() {
        fn walk(command: &clap::Command, path: &str, leaves: &mut Vec<String>) {
            let path = format!("{path} {}", command.get_name());
            if command.get_subcommands().next().is_none() {
                assert!(command.get_about().is_some(), "{path} has no description");
                let help = command
                    .get_after_help()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                assert!(
                    help.contains("Examples:\n  stackly "),
                    "{path} has no example"
                );
                leaves.push(path);
            } else {
                for sub in command.get_subcommands() {
                    if sub.get_name() != "help" {
                        walk(sub, &path, leaves);
                    }
                }
            }
        }
        let mut leaves = Vec::new();
        walk(&cli_command(None), "", &mut leaves);
        assert!(
            leaves.contains(&" stackly daemon status".to_owned()),
            "{leaves:?}"
        );
    }
}
