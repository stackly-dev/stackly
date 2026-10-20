//! Command groups (spec 0008, *Perintah async dan menambah command group*).
//! A group is one module here plus one variant of `app::Command`; each verb
//! implements [`RunCommand`], and [`execute`] writes its output. A group
//! never changes `output`, `exit`, `session`, `wait`, `confirm`, or
//! `progress`.

pub mod daemon;
pub mod operation;
pub mod secret;

use std::future::Future;

use serde::Serialize;

use crate::context::Context;
use crate::output::CommandOutput;
use crate::output::human::Render;

/// One verb of a command group.
pub trait RunCommand {
    /// The result: the `--json` `result` and the text for people.
    type Output: Serialize + Render + Send;

    /// Runs the verb. It never writes to stdout and never exits the
    /// process; its [`CommandOutput`] decides the envelope and the exit
    /// code.
    fn run(self, cx: &mut Context) -> impl Future<Output = CommandOutput<Self::Output>> + Send;
}

/// Runs `command` and writes its output; returns the exit code.
pub async fn execute<C: RunCommand + Send>(command: C, cx: &mut Context) -> u8 {
    let output = command.run(cx).await;
    cx.output.emit(output)
}
