//! The equivalent `stackly instance create` command (spec 0020, *Perintah
//! setara*): [`CommandDraft`] is what the daemon says the command does, and
//! [`equivalent_command`] writes it as text. The text never holds a secret
//! value; a secret field shows only where its value comes from.
//!
//! The CLI reads the text back with the same rules and compares the draft
//! it gets with the draft that made the text, which is the round trip test
//! of AC-18.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::PortRequest;
use super::create::{SecretSource, SettingValue};

/// What a create command does, in the order of its flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandDraft {
    /// The instance name, as typed.
    pub name: String,
    /// `--runtime`: `software@version` when exactly one installed runtime
    /// matches it, else the runtime ID.
    pub runtime: String,
    /// `--provider`, only when the draft names one.
    #[serde(default)]
    pub provider: Option<String>,
    /// `--revision`, only when the draft names one.
    #[serde(default)]
    pub revision: Option<String>,
    /// `--display-name`.
    #[serde(default)]
    pub display_name: Option<String>,
    /// One `--port` per endpoint, in the order of the form.
    pub ports: Vec<CommandPort>,
    /// One `--set` or `--set-json` per setting that differs from its
    /// default, in the order of the form.
    pub settings: Vec<CommandSetting>,
    /// One `--secret` per secret field.
    pub secrets: Vec<CommandSecret>,
    /// `--start`.
    pub start: bool,
}

/// One `--port <endpoint>=<port|auto>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandPort {
    /// The endpoint name.
    pub endpoint: String,
    /// The port it asks for.
    pub port: PortRequest,
}

/// One `--set <field>=<value>`, or `--set-json` for a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandSetting {
    /// The settings field.
    pub field: String,
    /// Its value.
    pub value: SettingValue,
}

/// One `--secret <field>=<source>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandSecret {
    /// The settings field.
    pub field: String,
    /// Where its value comes from.
    pub source: SecretSource,
}

/// The command as one line of shell text: `stackly instance create <name>
/// --runtime <runtime>`, then `--provider` and `--revision` when set,
/// `--display-name`, a `--port` per endpoint, a `--set` or `--set-json` per
/// changed setting, a `--secret` per secret field, and `--start`. A word with
/// a character outside `[A-Za-z0-9._/:@%+=,-]` is single quoted the POSIX
/// way. A `value` secret is written as `prompt`, because the command can
/// not carry the value.
#[must_use]
pub fn equivalent_command(draft: &CommandDraft) -> String {
    let mut words: Vec<String> = vec![
        "stackly".to_owned(),
        "instance".to_owned(),
        "create".to_owned(),
        quote(&draft.name),
        "--runtime".to_owned(),
        quote(&draft.runtime),
    ];
    let mut flag = |name: &str, value: &str| {
        words.push(name.to_owned());
        words.push(quote(value));
    };
    if let Some(provider) = &draft.provider {
        flag("--provider", provider);
    }
    if let Some(revision) = &draft.revision {
        flag("--revision", revision);
    }
    if let Some(display_name) = &draft.display_name {
        flag("--display-name", display_name);
    }
    for port in &draft.ports {
        let text = match port.port {
            PortRequest::Fixed(port_number) => port_number.to_string(),
            PortRequest::Auto => "auto".to_owned(),
        };
        flag("--port", &format!("{}={text}", port.endpoint));
    }
    for setting in &draft.settings {
        match &setting.value {
            SettingValue::Text(text) => flag("--set", &format!("{}={text}", setting.field)),
            SettingValue::Integer(number) => flag("--set", &format!("{}={number}", setting.field)),
            SettingValue::Boolean(on) => flag("--set", &format!("{}={on}", setting.field)),
            SettingValue::List(_) => flag(
                "--set-json",
                &format!("{}={}", setting.field, setting.value.to_json()),
            ),
        }
    }
    for secret in &draft.secrets {
        let source = match &secret.source {
            SecretSource::Generate => "generate".to_owned(),
            SecretSource::Value => "prompt".to_owned(),
            SecretSource::CopyFrom(reference) => format!("copy:{reference}"),
            SecretSource::Bind(reference) => format!("bind:{reference}"),
        };
        flag("--secret", &format!("{}={source}", secret.field));
    }
    if draft.start {
        words.push("--start".to_owned());
    }
    words.join(" ")
}

/// Whether `word` can stand in a shell command without quotes.
fn is_plain(word: &str) -> bool {
    !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/:@%+=,-".contains(&b))
}

/// `word` as one POSIX shell word: unchanged when plain, else in single
/// quotes with each `'` written as `'\''`.
fn quote(word: &str) -> String {
    if is_plain(word) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SignedSafeInteger;
    use crate::instance::create::{ScalarSetting, SettingEntry, SettingItem};

    fn base() -> CommandDraft {
        CommandDraft {
            name: "orders-db".to_owned(),
            runtime: "postgresql@18.1".to_owned(),
            provider: None,
            revision: None,
            display_name: None,
            ports: Vec::new(),
            settings: Vec::new(),
            secrets: Vec::new(),
            start: false,
        }
    }

    #[test]
    fn the_smallest_command_names_the_instance_and_the_runtime() {
        assert_eq!(
            equivalent_command(&base()),
            "stackly instance create orders-db --runtime postgresql@18.1"
        );
    }

    #[test]
    fn flags_follow_the_order_of_the_spec_and_a_value_secret_is_written_as_prompt() {
        let draft = CommandDraft {
            provider: Some("official/postgresql".to_owned()),
            revision: Some("2026.10.1".to_owned()),
            display_name: Some("Orders DB".to_owned()),
            ports: vec![
                CommandPort {
                    endpoint: "postgres".to_owned(),
                    port: PortRequest::Fixed(5432),
                },
                CommandPort {
                    endpoint: "admin".to_owned(),
                    port: PortRequest::Auto,
                },
            ],
            settings: vec![
                CommandSetting {
                    field: "maxConnections".to_owned(),
                    value: SettingValue::Integer(SignedSafeInteger::new(50).unwrap()),
                },
                CommandSetting {
                    field: "jetstream".to_owned(),
                    value: SettingValue::Boolean(true),
                },
            ],
            secrets: vec![
                CommandSecret {
                    field: "adminPassword".to_owned(),
                    source: SecretSource::Value,
                },
                CommandSecret {
                    field: "replicaPassword".to_owned(),
                    source: SecretSource::CopyFrom("local:erp/db/replica".to_owned()),
                },
            ],
            start: true,
            ..base()
        };
        assert_eq!(
            equivalent_command(&draft),
            "stackly instance create orders-db --runtime postgresql@18.1 \
             --provider official/postgresql --revision 2026.10.1 --display-name 'Orders DB' \
             --port postgres=5432 --port admin=auto --set maxConnections=50 --set jetstream=true \
             --secret adminPassword=prompt --secret replicaPassword=copy:local:erp/db/replica \
             --start"
        );
    }

    #[test]
    fn a_word_outside_the_plain_set_is_single_quoted_with_embedded_quotes_escaped() {
        assert_eq!(quote("plain-1.2_3/x:y@z%+=,"), "plain-1.2_3/x:y@z%+=,");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote("$HOME;rm"), "'$HOME;rm'");
    }

    #[test]
    fn a_list_setting_is_compact_json_in_set_json() {
        let draft = CommandDraft {
            settings: vec![CommandSetting {
                field: "users".to_owned(),
                value: SettingValue::List(vec![SettingItem {
                    entries: vec![
                        SettingEntry {
                            key: "name".to_owned(),
                            value: ScalarSetting::Text("ann".to_owned()),
                        },
                        SettingEntry {
                            key: "limit".to_owned(),
                            value: ScalarSetting::Integer(SignedSafeInteger::new(-3).unwrap()),
                        },
                    ],
                }]),
            }],
            ..base()
        };
        assert_eq!(
            equivalent_command(&draft),
            "stackly instance create orders-db --runtime postgresql@18.1 \
             --set-json 'users=[{\"limit\":-3,\"name\":\"ann\"}]'"
        );
    }
}
