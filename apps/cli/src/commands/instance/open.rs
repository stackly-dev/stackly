//! `stackly instance open <instance> --list` (spec 0070, AC-12): the Open
//! In actions of an instance, one line each, read from
//! `instance.get_detail`.
//!
//! The command only lists. It never opens an application, writes a script,
//! or calls `secret.reveal`; opening is the GUI's job. Whether an app is
//! installed is not checked here either.

use clap::Args;
use serde::Serialize;
use stackly_protocol::instance::open_with::{open_action_label, shell_quote};
use stackly_protocol::instance::{
    ConnectionAvailability, ConnectionUnavailableReason, OpenAction, OpenActionKind,
};

use super::connection::InstanceRef;
use super::read_detail;
use super::read_instance;
use super::resolve::TargetArg;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{HumanWriter, Render};

/// The line for a provider without an `openWith` block.
const NO_ACTIONS: &str = "No open actions.";
/// The value of a row that cannot be opened.
const UNAVAILABLE_VALUE: &str = "—";
/// The note of an action that copies a password first.
const COPIES_PASSWORD: &str = "copies password";
/// The note of a kind this build does not know.
const UPDATE_TEXT: &str = "Update Stackly to use this action.";

/// `stackly instance open`.
#[derive(Debug, Args)]
pub struct OpenArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// List the actions without opening anything
    #[arg(long, required = true)]
    pub list: bool,
}

/// The `result` of `open --list`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOutput {
    /// The instance.
    pub instance: InstanceRef,
    /// The actions as the daemon described them, never with a secret value.
    pub open_actions: Vec<OpenAction>,
}

impl RunCommand for OpenArgs {
    type Output = OpenOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<OpenOutput> {
        match run(self, cx).await {
            Ok(output) => CommandOutput::ok(output),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

async fn run(args: OpenArgs, cx: &mut Context) -> Result<OpenOutput, CliFailure> {
    let (id, known) = args.instance.target.resolve_named(cx).await?;
    let detail = read_detail(cx, id).await?;
    let name = match known {
        Some(name) => name,
        None => read_instance(cx, id).await?.name,
    };
    Ok(OpenOutput {
        instance: InstanceRef { id, name },
        open_actions: detail.open_actions,
    })
}

/// The text of a reason the daemon gave (spec 0021, *Copy*).
fn reason_text(reason: Option<ConnectionUnavailableReason>) -> &'static str {
    match reason {
        Some(ConnectionUnavailableReason::PortNotAssigned) => {
            "Start the instance to assign its port."
        }
        Some(ConnectionUnavailableReason::PackageUnavailable) => {
            "Stackly can’t read this provider’s package. Run stackly doctor."
        }
        Some(ConnectionUnavailableReason::Retained) => "This instance was removed.",
        Some(ConnectionUnavailableReason::Unrecognized) | None => UPDATE_TEXT,
    }
}

/// The columns of one action: name, label, value, note.
pub(crate) fn row(action: &OpenAction) -> [String; 4] {
    let label = open_action_label(action.kind, &action.title);
    if action.kind == OpenActionKind::Unrecognized {
        return [
            action.name.clone(),
            label,
            UNAVAILABLE_VALUE.to_owned(),
            UPDATE_TEXT.to_owned(),
        ];
    }
    if action.state != ConnectionAvailability::Available {
        return [
            action.name.clone(),
            label,
            UNAVAILABLE_VALUE.to_owned(),
            reason_text(action.reason).to_owned(),
        ];
    }
    let value = match action.kind {
        OpenActionKind::Terminal => action
            .command
            .as_ref()
            .map(|command| {
                std::iter::once(&command.executable)
                    .chain(&command.args)
                    .map(|value| shell_quote(value))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default(),
        OpenActionKind::Reveal => action.path.clone().unwrap_or_default(),
        _ => action.url.clone().unwrap_or_default(),
    };
    let note = if action.copy_secret.is_some() {
        COPIES_PASSWORD
    } else {
        ""
    };
    [action.name.clone(), label, value, note.to_owned()]
}

/// The lines of `rows`: columns joined by two spaces and padded to the
/// longest cell of their column, with no trailing spaces.
pub(crate) fn lines(rows: &[[String; 4]]) -> Vec<String> {
    let mut widths = [0usize; 4];
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    rows.iter()
        .map(|row| {
            let mut line = String::new();
            for (index, cell) in row.iter().enumerate() {
                if index > 0 {
                    line.push_str("  ");
                }
                line.push_str(cell);
                if index + 1 < row.len() {
                    let pad = widths[index].saturating_sub(cell.chars().count());
                    line.extend(std::iter::repeat_n(' ', pad));
                }
            }
            line.trim_end().to_owned()
        })
        .collect()
}

impl Render for OpenOutput {
    fn render(&self, out: &mut HumanWriter) {
        if self.open_actions.is_empty() {
            out.line(NO_ACTIONS);
            return;
        }
        let rows: Vec<[String; 4]> = self.open_actions.iter().map(row).collect();
        for line in lines(&rows) {
            out.line(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stackly_protocol::instance::OpenCommand;
    use stackly_protocol::instance::connection::ConnectionSecretPart;

    fn action(kind: OpenActionKind, title: &str) -> OpenAction {
        OpenAction {
            name: title.to_lowercase(),
            title: title.to_owned(),
            kind,
            state: ConnectionAvailability::Available,
            reason: None,
            app_bundle_ids: Vec::new(),
            url: None,
            command: None,
            path: None,
            copy_secret: None,
        }
    }

    #[test]
    fn rows_show_the_value_of_each_kind_and_the_password_note() {
        let mut app = action(OpenActionKind::App, "TablePlus");
        app.url = Some("postgresql://postgres@127.0.0.1:5432/postgres".to_owned());
        app.copy_secret = Some(ConnectionSecretPart {
            field: "superuserPassword".to_owned(),
            reference: "local:instance/x/superuser-password".to_owned(),
        });
        let mut terminal = action(OpenActionKind::Terminal, "psql");
        terminal.command = Some(OpenCommand {
            executable: "/rt/bin/psql".to_owned(),
            args: vec!["--host=/a dir".to_owned()],
            env: Vec::new(),
        });
        let mut reveal = action(OpenActionKind::Reveal, "Routes File");
        reveal.path = Some("/i/config/routes.yaml".to_owned());

        assert_eq!(
            row(&app),
            [
                "tableplus",
                "Open in TablePlus",
                "postgresql://postgres@127.0.0.1:5432/postgres",
                "copies password"
            ]
        );
        assert_eq!(row(&terminal)[2], "'/rt/bin/psql' '--host=/a dir'");
        assert_eq!(row(&reveal)[1], "Reveal Routes File in Finder");
        assert_eq!(row(&reveal)[2], "/i/config/routes.yaml");
    }

    #[test]
    fn an_unavailable_or_unknown_row_has_a_dash_and_its_reason() {
        let mut retained = action(OpenActionKind::Browser, "Monitoring");
        retained.state = ConnectionAvailability::Unavailable;
        retained.reason = Some(ConnectionUnavailableReason::Retained);
        let unknown = action(OpenActionKind::Unrecognized, "Future");

        assert_eq!(
            row(&retained),
            [
                "monitoring",
                "Open Monitoring in Browser",
                "—",
                "This instance was removed."
            ]
        );
        assert_eq!(row(&unknown)[3], "Update Stackly to use this action.");
    }

    #[derive(serde::Deserialize)]
    struct Case {
        action: OpenAction,
        row: [String; 4],
    }

    #[derive(serde::Deserialize)]
    struct Cases {
        cases: Vec<Case>,
    }

    #[test]
    fn every_shared_case_gives_its_row() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/instance/detail-open-actions.json");
        let text = std::fs::read_to_string(path).unwrap();
        let cases: Cases = serde_json::from_str(&text).unwrap();
        for case in cases.cases {
            assert_eq!(row(&case.action), case.row, "{}", case.action.name);
        }
    }

    #[test]
    fn columns_are_left_aligned_with_two_spaces_and_no_trailing_space() {
        let rows = [
            [
                "a".to_owned(),
                "Open in A".to_owned(),
                "x://1".to_owned(),
                String::new(),
            ],
            [
                "bbb".to_owned(),
                "Open B".to_owned(),
                "x://22".to_owned(),
                "copies password".to_owned(),
            ],
        ];
        assert_eq!(
            lines(&rows),
            [
                "a    Open in A  x://1",
                "bbb  Open B     x://22  copies password"
            ]
        );
    }
}
