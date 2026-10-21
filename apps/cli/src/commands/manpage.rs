//! `stackly manpage` (spec 0015, AC-12), a hidden command: the man pages of
//! `stackly` and each of its commands, written from the clap tree by
//! `clap_mangen`. `cargo xtask build-app` puts them into the bundle.

use clap::Args;
use serde::Serialize;

use super::RunCommand;
use crate::app::cli_command;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};

/// `stackly manpage` takes no arguments.
#[derive(Debug, Args)]
pub struct ManpageArgs {}

/// One man page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Page {
    /// The file name, such as `stackly-daemon-status.1`.
    pub file: String,
    /// The page in roff.
    pub roff: String,
}

/// The `manpage` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManpageResult {
    /// Every page, the main one first.
    pub pages: Vec<Page>,
}

impl Render for ManpageResult {
    fn render(&self, out: &mut HumanWriter) {
        for page in &self.pages {
            out.line(&format!(".\\\" {}", page.file));
            for line in page.roff.lines() {
                out.line(line);
            }
        }
    }
}

/// Every man page of the `stackly` command tree.
#[must_use]
pub fn pages() -> Vec<Page> {
    fn walk(command: &clap::Command, name: &str, pages: &mut Vec<Page>) {
        let mut roff = Vec::new();
        let page = command.clone().name(name.to_owned());
        if clap_mangen::Man::new(page).render(&mut roff).is_ok() {
            pages.push(Page {
                file: format!("{name}.1"),
                roff: String::from_utf8_lossy(&roff).into_owned(),
            });
        }
        for sub in command.get_subcommands() {
            if sub.is_hide_set() || sub.get_name() == "help" {
                continue;
            }
            walk(sub, &format!("{name}-{}", sub.get_name()), pages);
        }
    }
    let mut root = cli_command(None);
    root.build();
    let mut pages = Vec::new();
    walk(&root, "stackly", &mut pages);
    pages
}

impl RunCommand for ManpageArgs {
    type Output = ManpageResult;

    async fn run(self, _cx: &mut Context) -> CommandOutput<ManpageResult> {
        CommandOutput::ok(ManpageResult { pages: pages() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_a_page_for_stackly_and_for_each_visible_command() {
        let files: Vec<String> = pages().into_iter().map(|page| page.file).collect();
        for expected in [
            "stackly.1",
            "stackly-daemon.1",
            "stackly-daemon-status.1",
            "stackly-daemon-restart.1",
            "stackly-completion.1",
        ] {
            assert!(
                files.iter().any(|file| file == expected),
                "{expected} in {files:?}"
            );
        }
        assert!(!files.iter().any(|file| file.contains("manpage")));
    }

    #[test]
    fn a_page_is_roff() {
        let pages = pages();
        assert!(pages[0].roff.contains(".TH"), "{}", pages[0].roff);
    }
}
