//! Confirmations (spec 0008, *Mode interaktif dan konfirmasi*). The
//! approval flag comes from the calling command, `--yes` through
//! [`ConfirmArgs`] by default, so only commands that confirm accept it.
//! The default answer is always no: Enter never runs the action.

use std::io::BufRead;

use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::style::Tone;

/// How heavy the action is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmRole {
    /// A normal confirmation.
    Standard,
    /// An action that removes or gives up something: the question is
    /// written with the critical tone.
    Destructive,
}

/// What to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmRequest<'a> {
    /// How heavy the action is.
    pub role: ConfirmRole,
    /// The question, such as `abandon operation <id>?`.
    pub question: &'a str,
    /// What happens when the answer is yes.
    pub consequence: &'a str,
}

/// The calling command's approval flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Approval {
    /// Whether the flag was given.
    pub given: bool,
    /// The flag, for `details.flag` of `cli.confirmation_required`.
    pub flag: &'static str,
}

/// What to do without a terminal and without the approval flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnNoninteractive {
    /// Fail `cli.confirmation_required` before anything reaches the daemon.
    Fail,
    /// Go on unconfirmed, so the daemon answers with its own confirmation
    /// code (such as `secret.confirmation_required`, spec 0011).
    LetDaemonDecide,
}

/// The outcome of [`Confirm::ask`] that lets the command go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    /// Approved by the flag or by the person at the terminal.
    Confirmed,
    /// Not asked: noninteractive with [`OnNoninteractive::LetDaemonDecide`].
    LeftToDaemon,
}

/// The default approval flag, `--yes` or `-y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::Args)]
pub struct ConfirmArgs {
    /// Confirm without asking; needed when stackly runs noninteractive.
    #[arg(short = 'y', long)]
    pub yes: bool,
}

impl ConfirmArgs {
    /// The approval this flag gives.
    #[must_use]
    pub const fn approval(&self) -> Approval {
        Approval {
            given: self.yes,
            flag: "--yes",
        }
    }
}

/// Asks for confirmations.
#[derive(Debug)]
pub struct Confirm;

impl Confirm {
    /// Confirms `request`. A given approval passes at once. Noninteractive
    /// without it, `on_noninteractive` decides, and stdin is never read.
    /// Interactive, the question, the consequence, and `[y/N] ` go to
    /// stderr and one line is read from stdin; only `y` or `yes`, in any
    /// case, confirms.
    ///
    /// # Errors
    ///
    /// `cli.confirmation_required` noninteractive with
    /// [`OnNoninteractive::Fail`], and `cli.confirmation_declined` for any
    /// other answer, Enter and end of input included.
    pub fn ask(
        cx: &mut Context,
        request: ConfirmRequest<'_>,
        approval: Approval,
        on_noninteractive: OnNoninteractive,
    ) -> Result<Confirmation, CliFailure> {
        if approval.given {
            return Ok(Confirmation::Confirmed);
        }
        if !cx.prompt_allowed() {
            return match on_noninteractive {
                OnNoninteractive::Fail => Err(CliFailure::confirmation_required(approval.flag)),
                OnNoninteractive::LetDaemonDecide => Ok(Confirmation::LeftToDaemon),
            };
        }
        let sink = &mut cx.output.stderr;
        let question = match request.role {
            ConfirmRole::Standard => request.question.to_owned(),
            ConfirmRole::Destructive => sink.styled(request.question, Tone::Critical.style()),
        };
        sink.line(&question);
        sink.line(request.consequence);
        sink.prompt("[y/N] ");
        let mut answer = String::new();
        let read = cx.input.read_line(&mut answer);
        match read {
            Ok(n)
                if n > 0 && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") =>
            {
                Ok(Confirmation::Confirmed)
            }
            Ok(_) | Err(_) => Err(CliFailure::confirmation_declined()),
        }
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::codes::cli;

    use super::*;
    use crate::context::testing::context;

    const REQUEST: ConfirmRequest<'static> = ConfirmRequest {
        role: ConfirmRole::Destructive,
        question: "abandon operation 1?",
        consequence: "its leftovers stay on disk",
    };

    fn approval(given: bool, flag: &'static str) -> Approval {
        Approval { given, flag }
    }

    #[test]
    fn only_y_or_yes_in_any_case_confirms() {
        for (input, confirmed) in [
            ("y\n", true),
            ("YES\n", true),
            ("  yes  \n", true),
            ("Y", true),
            ("\n", false),
            ("", false),
            ("n\n", false),
            ("yep\n", false),
            ("no\n", false),
        ] {
            let (mut cx, stderr) = context(true, input);
            let answer = Confirm::ask(
                &mut cx,
                REQUEST,
                approval(false, "--yes"),
                OnNoninteractive::Fail,
            );
            if confirmed {
                assert_eq!(answer, Ok(Confirmation::Confirmed), "{input:?}");
            } else {
                let failure = answer.unwrap_err();
                assert!(failure.body.is(&cli::CONFIRMATION_DECLINED), "{input:?}");
                assert_eq!(failure.exit_code(), 9);
            }
            assert_eq!(
                stderr.text(),
                "abandon operation 1?\nits leftovers stay on disk\n[y/N] "
            );
        }
    }

    #[test]
    fn a_given_approval_passes_without_asking_in_both_modes() {
        for interactive in [true, false] {
            let (mut cx, stderr) = context(interactive, "n\n");
            for flag in ["--yes", "--confirm-stored-only"] {
                assert_eq!(
                    Confirm::ask(
                        &mut cx,
                        REQUEST,
                        approval(true, flag),
                        OnNoninteractive::Fail
                    ),
                    Ok(Confirmation::Confirmed)
                );
            }
            assert_eq!(stderr.text(), "");
        }
    }

    #[test]
    fn noninteractive_without_approval_fails_with_the_flag_or_leaves_it_to_the_daemon() {
        let (mut cx, stderr) = context(false, "y\n");
        let failure = Confirm::ask(
            &mut cx,
            REQUEST,
            approval(false, "--confirm-stored-only"),
            OnNoninteractive::Fail,
        )
        .unwrap_err();
        assert!(failure.body.is(&cli::CONFIRMATION_REQUIRED));
        assert_eq!(failure.body.detail("flag"), Some("--confirm-stored-only"));
        assert_eq!(failure.exit_code(), 2);
        assert_eq!(
            Confirm::ask(
                &mut cx,
                REQUEST,
                approval(false, "--yes"),
                OnNoninteractive::LetDaemonDecide
            ),
            Ok(Confirmation::LeftToDaemon)
        );
        assert_eq!(stderr.text(), "");
        let mut unread = String::new();
        cx.input.read_line(&mut unread).unwrap();
        assert_eq!(unread, "y\n", "stdin was never read");
    }

    #[test]
    fn a_standard_question_is_not_colored() {
        let (mut cx, stderr) = context(true, "y\n");
        let request = ConfirmRequest {
            role: ConfirmRole::Standard,
            ..REQUEST
        };
        Confirm::ask(
            &mut cx,
            request,
            approval(false, "--yes"),
            OnNoninteractive::Fail,
        )
        .unwrap();
        assert!(stderr.text().starts_with("abandon operation 1?\n"));
    }

    #[test]
    fn the_default_flag_is_yes() {
        assert_eq!(
            ConfirmArgs { yes: true }.approval(),
            approval(true, "--yes")
        );
    }
}
