//! [`Context`], what every command runs with (spec 0008): the output, the
//! interactive decision made once at start, stdin for confirmations, the
//! progress mode, and the lazy daemon session.

use std::io::BufRead;

use stackly_ipc::EventStream;
use stackly_protocol::events::{SubscribeParams, Subscribed};
use stackly_protocol::handshake::HandshakeAccepted;
use stackly_protocol::{Call, Reply};

use crate::failure::CliFailure;
use crate::output::Output;
use crate::output::envelope::Warning;
use crate::progress::ProgressMode;
use crate::session::{CallSpec, Connector, Session};

/// What a command runs with.
pub struct Context {
    /// Where and how the output goes.
    pub output: Output,
    /// stdin and stderr are terminals, and neither `--no-input` nor
    /// `STACKLY_NO_INPUT` is on.
    pub interactive: bool,
    /// Where confirmations are read from.
    pub input: Box<dyn BufRead + Send>,
    /// How progress is shown while waiting.
    pub progress: ProgressMode,
    connector: Connector,
    session: Option<Session>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("output", &self.output)
            .field("interactive", &self.interactive)
            .field("progress", &self.progress)
            .field("connector", &self.connector)
            .finish_non_exhaustive()
    }
}

impl Context {
    /// A context that connects through `connector` on its first call.
    #[must_use]
    pub fn new(
        output: Output,
        interactive: bool,
        input: Box<dyn BufRead + Send>,
        progress: ProgressMode,
        connector: Connector,
    ) -> Self {
        Self {
            output,
            interactive,
            input,
            progress,
            connector,
            session: None,
        }
    }

    /// Whether code may ask the person at the terminal anything.
    #[must_use]
    pub const fn prompt_allowed(&self) -> bool {
        self.interactive
    }

    /// Fails `cli.input_required` for `input` when no prompt is allowed,
    /// without reading stdin.
    ///
    /// # Errors
    ///
    /// `cli.input_required` in noninteractive mode.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "spec 0011 step 8 asks for secrets through it")
    )]
    pub fn require_prompt(&self, input: &str) -> Result<(), CliFailure> {
        if self.prompt_allowed() {
            Ok(())
        } else {
            Err(CliFailure::input_required(input))
        }
    }

    /// Adds a warning for the output.
    pub fn warn(&mut self, code: &str, message: impl Into<String>) {
        self.output.warnings.push(Warning::new(code, message));
    }

    /// The handshake answer of the session, connecting first if needed.
    ///
    /// # Errors
    ///
    /// The errors of [`Session::connect`].
    pub async fn handshake(&mut self) -> Result<HandshakeAccepted, CliFailure> {
        Ok(self.session().await?.accepted().clone())
    }

    /// Sends `call` with [`Session::call`], connecting first if needed.
    ///
    /// # Errors
    ///
    /// The errors of [`Session::connect`] and [`Session::call`].
    pub async fn call(&mut self, call: Call, spec: CallSpec) -> Result<Reply, CliFailure> {
        self.connect().await?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(CliFailure::not_connected)?;
        session
            .call(&call, &spec, &mut self.output.stderr, self.output.verbose)
            .await
    }

    /// Sends `call` once, without the `daemon.starting` retry, connecting
    /// first if needed.
    ///
    /// # Errors
    ///
    /// The errors of [`Session::connect`] and [`Session::call_once`].
    pub async fn call_once(&mut self, call: Call, spec: CallSpec) -> Result<Reply, CliFailure> {
        self.connect().await?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(CliFailure::not_connected)?;
        session
            .call_once(call, &spec, &mut self.output.stderr, self.output.verbose)
            .await
    }

    /// Starts an event subscription, connecting first if needed.
    ///
    /// # Errors
    ///
    /// The errors of [`Session::connect`] and [`Session::subscribe`].
    pub async fn subscribe(
        &mut self,
        params: SubscribeParams,
    ) -> Result<(Subscribed, EventStream), CliFailure> {
        self.connect().await?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(CliFailure::not_connected)?;
        session
            .subscribe(params, &mut self.output.stderr, self.output.verbose)
            .await
    }

    /// Drops the session, so the next call connects again.
    pub fn disconnect(&mut self) {
        self.session = None;
    }

    async fn session(&mut self) -> Result<&Session, CliFailure> {
        self.connect().await?;
        self.session.as_ref().ok_or_else(CliFailure::not_connected)
    }

    async fn connect(&mut self) -> Result<(), CliFailure> {
        if self.session.is_none() {
            let session = Session::connect(
                &self.connector,
                &mut self.output.stderr,
                self.output.verbose,
            )
            .await?;
            self.session = Some(session);
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod testing {
    //! A [`Context`] over captured streams and a socket nobody listens on.

    use std::io::Cursor;
    use std::path::PathBuf;

    use super::*;
    use crate::output::OutputMode;
    use crate::output::stderr::Captured;
    use crate::output::testing::output;

    /// A text mode context, interactive or not, reading `input`, and its
    /// captured stderr.
    pub fn context(interactive: bool, input: &str) -> (Context, Captured) {
        let (output, _, stderr) = output(OutputMode::Human);
        let cx = Context::new(
            output,
            interactive,
            Box::new(Cursor::new(input.as_bytes().to_vec())),
            ProgressMode::Off,
            Connector {
                socket: PathBuf::from("/nonexistent/stacklyd.sock"),
                uid: 0,
                interactive,
            },
        );
        (cx, stderr)
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::codes::{cli, ipc};

    use super::testing::context;
    use super::*;

    #[test]
    fn a_prompt_in_noninteractive_mode_fails_at_once_though_stdin_is_open() {
        let (cx, stderr) = context(false, "answer\n");
        let failure = cx.require_prompt("secret value").unwrap_err();
        assert!(failure.body.is(&cli::INPUT_REQUIRED));
        assert_eq!(failure.body.detail("input"), Some("secret value"));
        assert_eq!(failure.exit_code(), 2);
        assert_eq!(stderr.text(), "");
        let (cx, _) = context(true, "");
        assert!(cx.prompt_allowed());
        assert_eq!(cx.require_prompt("secret value"), Ok(()));
    }

    #[tokio::test]
    async fn a_call_without_a_daemon_fails_with_the_transport_error_and_no_request_id() {
        let (mut cx, _) = context(false, "");
        let failure = cx
            .call(Call::DaemonGetInfo, CallSpec::control())
            .await
            .unwrap_err();
        assert!(failure.body.is(&ipc::ENDPOINT_MISSING), "{failure:?}");
        assert_eq!(failure.request_id, None);
        assert_eq!(failure.exit_code(), 6);
    }
}
