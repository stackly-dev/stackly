//! `stackly completion <bash|zsh|fish>` (spec 0015, AC-12): the shell
//! completion script, written from the clap tree.

use clap::{Args, ValueEnum};
use clap_complete::Shell;
use serde::Serialize;

use super::RunCommand;
use crate::app::cli_command;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};

/// The shells that get a completion script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ShellName {
    /// bash
    Bash,
    /// zsh
    Zsh,
    /// fish
    Fish,
}

impl ShellName {
    const fn shell(self) -> Shell {
        match self {
            Self::Bash => Shell::Bash,
            Self::Zsh => Shell::Zsh,
            Self::Fish => Shell::Fish,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
        }
    }
}

/// `stackly completion`.
#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// The shell to write the script for
    #[arg(value_enum)]
    pub shell: ShellName,
}

/// The `completion` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionResult {
    /// The shell.
    pub shell: &'static str,
    /// The script.
    pub script: String,
}

impl Render for CompletionResult {
    fn render(&self, out: &mut HumanWriter) {
        for line in self.script.lines() {
            out.line(line);
        }
    }
}

/// The completion script of `shell` for `stackly`.
#[must_use]
pub fn script(shell: ShellName) -> String {
    let mut command = cli_command(None);
    let mut script = Vec::new();
    clap_complete::generate(shell.shell(), &mut command, "stackly", &mut script);
    String::from_utf8_lossy(&script).into_owned()
}

impl RunCommand for CompletionArgs {
    type Output = CompletionResult;

    async fn run(self, _cx: &mut Context) -> CommandOutput<CompletionResult> {
        CommandOutput::ok(CompletionResult {
            shell: self.shell.name(),
            script: script(self.shell),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shell_gets_a_script_that_names_the_command_and_a_daemon_verb() {
        for shell in [ShellName::Bash, ShellName::Zsh, ShellName::Fish] {
            let script = script(shell);
            assert!(script.contains("stackly"), "{}", shell.name());
            assert!(script.contains("restart"), "{}", shell.name());
        }
    }
}
