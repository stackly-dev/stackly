//! The Open In actions of an instance (spec 0070): the `openActions` of
//! `instance.get_detail`, the params and reply of
//! `instance.prepare_terminal`, and the pure helpers both the daemon and the
//! CLI use, [`shell_quote`], [`terminal_script`], and [`open_action_label`].
//!
//! No DTO here holds a secret value. An action that copies a password
//! carries the field and its reference, and the GUI reads the value through
//! `secret.reveal` only when the user picks the action.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use super::connection::ConnectionSecretPart;
use super::detail::{ConnectionAvailability, ConnectionUnavailableReason};
use crate::error::FieldError;
use crate::json::{FieldCode, Validate};

/// Longest action name, in bytes.
pub const OPEN_ACTION_NAME_MAX: usize = 32;

/// The kind of an Open In action. Open.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpenActionKind {
    /// Opens `url` with the first installed app of `appBundleIds`.
    App,
    /// Runs `command` in Terminal through `instance.prepare_terminal`.
    Terminal,
    /// Opens `url` in the default browser.
    Browser,
    /// Shows `path` in Finder.
    Reveal,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

/// One env pair of a terminal command.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvPair {
    /// The env name.
    pub name: String,
    /// The value.
    pub value: String,
}

/// The command of a terminal action.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenCommand {
    /// The absolute path of the executable in the instance's runtime.
    pub executable: String,
    /// The arguments, each one value.
    pub args: Vec<String>,
    /// The env pairs from the manifest, in name order.
    #[serde(default)]
    pub env: Vec<EnvPair>,
}

/// One Open In action of an instance, in manifest order.
///
/// When `state` is `available`, `reason` is null and the field of the kind
/// is set: `url` for `app` and `browser` (with `appBundleIds` for `app`),
/// `command` for `terminal`, and `path` for `reveal`. When it is
/// `unavailable`, `reason` is set and none of them is.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenAction {
    /// The entry name from the manifest, such as `psql`.
    pub name: String,
    /// The title the label is built from.
    pub title: String,
    /// The kind of the action.
    pub kind: OpenActionKind,
    /// Whether the action can be opened, as far as the daemon knows.
    pub state: ConnectionAvailability,
    /// Set if and only if the action is unavailable.
    #[serde(default)]
    pub reason: Option<ConnectionUnavailableReason>,
    /// The bundle IDs of an `app` action, in priority order.
    #[serde(default)]
    pub app_bundle_ids: Vec<String>,
    /// The URL of an available `app` or `browser` action.
    #[serde(default)]
    pub url: Option<String>,
    /// The command of an available `terminal` action.
    #[serde(default)]
    pub command: Option<OpenCommand>,
    /// The absolute path of an available `reveal` action.
    #[serde(default)]
    pub path: Option<String>,
    /// The secret the GUI copies to the hidden clipboard before it opens
    /// the action: the field and the reference, never the value.
    #[serde(default)]
    pub copy_secret: Option<ConnectionSecretPart>,
}

/// Params of `instance.prepare_terminal`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrepareTerminalParams {
    /// The instance.
    pub instance_id: Uuid,
    /// The name of a `terminal` action of the instance.
    pub action: String,
}

impl Validate for PrepareTerminalParams {
    /// `action` follows the entry name rule `^[a-z][a-z0-9_]{0,31}$`.
    fn validate(&self) -> Vec<FieldError> {
        if self.action.is_empty() {
            vec![FieldError::new("action", FieldCode::Required)]
        } else if self.action.len() > OPEN_ACTION_NAME_MAX {
            vec![FieldError::new("action", FieldCode::TooLong)]
        } else if !is_action_name(&self.action) {
            vec![FieldError::new("action", FieldCode::InvalidFormat)]
        } else {
            Vec::new()
        }
    }
}

/// Whether `name` matches `^[a-z][a-z0-9_]{0,31}$`.
fn is_action_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    matches!(bytes.first(), Some(b'a'..=b'z'))
        && bytes.len() <= OPEN_ACTION_NAME_MAX
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// Reply of `instance.prepare_terminal`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrepareTerminalReply {
    /// The absolute path of the `.command` script the GUI opens with
    /// Terminal.
    pub script_path: String,
}

/// The menu label of an action (spec 0070, *Label aksi*): "Open in
/// {title}", "Open {title} in Terminal", "Open {title} in Browser", or
/// "Reveal {title} in Finder". An unrecognized kind gets the title alone.
#[must_use]
pub fn open_action_label(kind: OpenActionKind, title: &str) -> String {
    match kind {
        OpenActionKind::App => format!("Open in {title}"),
        OpenActionKind::Terminal => format!("Open {title} in Terminal"),
        OpenActionKind::Browser => format!("Open {title} in Browser"),
        OpenActionKind::Reveal => format!("Reveal {title} in Finder"),
        OpenActionKind::Unrecognized => title.to_owned(),
    }
}

/// Quotes a value for a POSIX shell: the whole value in single quotes, and
/// each `'` written as `'\''`.
#[must_use]
pub fn shell_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// The env names the script passes on from the user's session; every other
/// variable is dropped by `env -i`.
const PASSED_ENV: [&str; 5] = ["HOME", "USER", "LOGNAME", "TERM", "LANG"];

/// Whether `name` matches `^[A-Za-z_][A-Za-z0-9_]*$`, so it stands in the
/// script unquoted as the name of an `env` assignment and nothing else.
fn is_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The `.command` script of a terminal action (spec 0070, *Skrip
/// Terminal*): `env -i` with the user's `HOME`, `USER`, `LOGNAME`, `TERM`,
/// and `LANG` and the manifest env, then the absolute executable with its
/// args, every value through [`shell_quote`]. An env pair whose name is
/// not `^[A-Za-z_][A-Za-z0-9_]*$` is left out, because a name is written
/// unquoted; the package loader refuses such a name already, so this only
/// guards a caller that builds the [`OpenCommand`] another way. A non zero exit prints the
/// status and waits for Return, so the window stays open. The script holds
/// the instance ID, never its name or a secret.
#[must_use]
pub fn terminal_script(instance_id: Uuid, title: &str, command: &OpenCommand) -> String {
    let mut out = String::new();
    out.push_str("#!/bin/sh\n");
    out.push_str(&format!(
        "# Written by Stackly for instance {instance_id}. Holds no secret.\n"
    ));
    out.push_str("/usr/bin/env -i");
    for name in PASSED_ENV {
        out.push_str(&format!(" {name}=\"${name}\""));
    }
    out.push_str(" \\\n");
    let env: Vec<&EnvPair> = command
        .env
        .iter()
        .filter(|pair| is_env_name(&pair.name))
        .collect();
    if !env.is_empty() {
        out.push(' ');
        for pair in env {
            out.push_str(&format!(" {}={}", pair.name, shell_quote(&pair.value)));
        }
        out.push_str(" \\\n");
    }
    out.push_str(&format!("  {}", shell_quote(&command.executable)));
    for arg in &command.args {
        out.push(' ');
        out.push_str(&shell_quote(arg));
    }
    out.push('\n');
    out.push_str("status=$?\n");
    out.push_str("if [ \"$status\" -ne 0 ]; then\n");
    out.push_str(&format!(
        "  printf '\\n%s exited with status %s. Press Return to close.\\n' {} \"$status\"\n",
        shell_quote(title)
    ));
    out.push_str("  read -r _\n");
    out.push_str("fi\n");
    out.push_str("exit \"$status\"\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_wraps_in_single_quotes_and_escapes_quotes() {
        assert_eq!(shell_quote("psql"), "'psql'");
        assert_eq!(shell_quote("/abs/run dir"), "'/abs/run dir'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("$HOME `x` \"y\""), "'$HOME `x` \"y\"'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn labels_follow_the_kind() {
        assert_eq!(
            open_action_label(OpenActionKind::App, "TablePlus"),
            "Open in TablePlus"
        );
        assert_eq!(
            open_action_label(OpenActionKind::Terminal, "psql"),
            "Open psql in Terminal"
        );
        assert_eq!(
            open_action_label(OpenActionKind::Browser, "Monitoring"),
            "Open Monitoring in Browser"
        );
        assert_eq!(
            open_action_label(OpenActionKind::Reveal, "Routes File"),
            "Reveal Routes File in Finder"
        );
        assert_eq!(open_action_label(OpenActionKind::Unrecognized, "X"), "X");
    }

    #[test]
    fn action_names_follow_the_entry_rule() {
        let params = |action: &str| PrepareTerminalParams {
            instance_id: Uuid::nil(),
            action: action.to_owned(),
        };
        assert!(params("psql").validate().is_empty());
        assert_eq!(
            params("").validate()[0].code(),
            FieldCode::Required.as_str()
        );
        assert_eq!(
            params(&"a".repeat(33)).validate()[0].code(),
            FieldCode::TooLong.as_str()
        );
        assert_eq!(
            params("Psql").validate()[0].code(),
            FieldCode::InvalidFormat.as_str()
        );
        assert_eq!(
            params("ps-ql").validate()[0].code(),
            FieldCode::InvalidFormat.as_str()
        );
    }

    #[test]
    fn the_terminal_script_runs_the_absolute_executable_with_a_clean_env() {
        let command = OpenCommand {
            executable: "/abs/path/runtime/bin/psql".to_owned(),
            args: vec![
                "--host=/abs/run dir".to_owned(),
                "--port=54321".to_owned(),
                "--username=postgres".to_owned(),
                "postgres".to_owned(),
            ],
            env: vec![EnvPair {
                name: "PGAPPNAME".to_owned(),
                value: "stackly-terminal".to_owned(),
            }],
        };
        let id: Uuid = "0192f000-0000-7000-8000-000000000001"
            .parse()
            .expect("the literal is a UUID");
        let script = terminal_script(id, "psql", &command);
        assert_eq!(
            script,
            "#!/bin/sh\n\
             # Written by Stackly for instance 0192f000-0000-7000-8000-000000000001. Holds no secret.\n\
             /usr/bin/env -i HOME=\"$HOME\" USER=\"$USER\" LOGNAME=\"$LOGNAME\" TERM=\"$TERM\" LANG=\"$LANG\" \\\n  \
             PGAPPNAME='stackly-terminal' \\\n  \
             '/abs/path/runtime/bin/psql' '--host=/abs/run dir' '--port=54321' '--username=postgres' 'postgres'\n\
             status=$?\n\
             if [ \"$status\" -ne 0 ]; then\n  \
             printf '\\n%s exited with status %s. Press Return to close.\\n' 'psql' \"$status\"\n  \
             read -r _\n\
             fi\n\
             exit \"$status\"\n"
        );
    }

    #[test]
    fn an_env_name_outside_the_shell_name_rule_is_left_out_of_the_script() {
        let command = OpenCommand {
            executable: "/abs/bin/tool".to_owned(),
            args: Vec::new(),
            env: vec![
                EnvPair {
                    name: "X;touch /tmp/pwned;Y".to_owned(),
                    value: "a".to_owned(),
                },
                EnvPair {
                    name: "_OK_1".to_owned(),
                    value: "b".to_owned(),
                },
                EnvPair {
                    name: "1BAD".to_owned(),
                    value: "c".to_owned(),
                },
                EnvPair {
                    name: String::new(),
                    value: "d".to_owned(),
                },
                EnvPair {
                    name: "$(id)".to_owned(),
                    value: "e".to_owned(),
                },
            ],
        };
        let script = terminal_script(Uuid::nil(), "tool", &command);
        assert!(script.contains(" _OK_1='b' \\\n"), "{script}");
        for bad in ["touch", "1BAD", "$(id)", "='d'"] {
            assert!(!script.contains(bad), "{bad} in {script}");
        }
    }

    #[test]
    fn a_script_whose_env_names_are_all_invalid_has_no_env_line() {
        let command = OpenCommand {
            executable: "/abs/bin/tool".to_owned(),
            args: Vec::new(),
            env: vec![EnvPair {
                name: "A B".to_owned(),
                value: "x".to_owned(),
            }],
        };
        let script = terminal_script(Uuid::nil(), "tool", &command);
        assert!(
            script.contains("LANG=\"$LANG\" \\\n  '/abs/bin/tool'\n"),
            "{script}"
        );
    }

    #[test]
    fn the_env_name_rule_accepts_a_letter_or_underscore_then_word_characters() {
        for good in ["A", "_", "PGAPPNAME", "lower_case9", "_9"] {
            assert!(is_env_name(good), "{good}");
        }
        for bad in ["", "9A", "A-B", "A B", "A=B", "\u{e9}", "A\n"] {
            assert!(!is_env_name(bad), "{bad:?}");
        }
    }
}
