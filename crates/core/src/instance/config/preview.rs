//! The Effective File of the Configuration page (spec 0029, *Evaluator
//! config*): the non sensitive config files a set of values renders,
//! without resolving a secret, and the lines a proposal changes.

use std::collections::BTreeMap;

use stackly_domain::{Endpoint, Instance, Port, PortRequest};
use stackly_platform::InstancePaths;

use crate::provider::{
    InitStep, PlanContext, PlanError, ResolvedEndpoint, RuntimeContext, Segment, SettingsValues,
    ValidatedPackage,
};

/// One config file as the Configuration page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveFile {
    /// The path relative to the instance's `config/` folder.
    pub path: String,
    /// The text.
    pub content: String,
    /// The 1 based lines of `content` a proposal changes; empty outside a
    /// preview of a change.
    pub changed_lines: Vec<u32>,
    /// Whether the text is a preview rendered from the values, rather than
    /// the copy of the active revision's last render.
    pub preview: bool,
    /// The note shown above the text when an `Auto` endpoint without a
    /// bound port is shown with its default port.
    pub port_note: Option<String>,
}

/// Renders the non sensitive config files in `config/` that `settings`
/// and the instance's endpoints give (spec 0029). Each IP endpoint takes
/// its bound port, else its fixed one, else the manifest's `defaultPort`
/// with a port note. A file outside `config/` or a sensitive one never
/// shows, so no secret is resolved or sent.
///
/// # Errors
///
/// Returns the [`PlanError`] of the run plan, for example settings that do
/// not validate.
pub fn preview(
    package: &ValidatedPackage,
    instance: &Instance,
    paths: &InstancePaths,
    runtime: &RuntimeContext,
    settings: &SettingsValues,
) -> Result<Vec<EffectiveFile>, PlanError> {
    let mut defaulted: Vec<Port> = Vec::new();
    let endpoints = instance
        .endpoints()
        .iter()
        .map(|endpoint| match endpoint {
            Endpoint::Ip(ip) => {
                let port = ip.effective_port().or_else(|| match ip.request() {
                    PortRequest::Auto => {
                        let fallback = package
                            .endpoints()
                            .find(|spec| spec.name() == ip.name())
                            .and_then(|spec| spec.default_port());
                        defaulted.extend(fallback);
                        fallback
                    }
                    PortRequest::Fixed(port) => Some(port),
                });
                (
                    ip.name().clone(),
                    ResolvedEndpoint::Ip {
                        host: ip.address(),
                        port,
                    },
                )
            }
            Endpoint::Unix(unix) => (unix.name().clone(), ResolvedEndpoint::Unix),
        })
        .collect::<BTreeMap<_, _>>();
    let context = PlanContext {
        instance_id: instance.id(),
        instance_name: instance.name().clone(),
        paths: paths.clone(),
        runtime: runtime.clone(),
        endpoints,
        settings: settings.clone(),
    };
    let plan = package.run_plan(&context)?;
    let port_note = (!defaulted.is_empty()).then(|| {
        let ports: Vec<String> = defaulted.iter().map(ToString::to_string).collect();
        format!("Automatic ports shown as {}", ports.join(", "))
    });
    let mut files = Vec::new();
    for step in plan.render {
        let InitStep::WriteFile(write) = step else {
            continue;
        };
        if write.sensitive {
            continue;
        }
        let Ok(relative) = write.path.strip_prefix(&paths.config) else {
            continue;
        };
        let Some(path) = relative.to_str() else {
            continue;
        };
        let content: String = write
            .content
            .segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Text(text) => Some(text.as_str()),
                Segment::Secret { .. } => None,
            })
            .collect();
        files.push(EffectiveFile {
            path: path.to_owned(),
            content,
            changed_lines: Vec::new(),
            preview: true,
            port_note: port_note.clone(),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The 1 based lines of `new` that are not in the longest common line
/// sequence of `old` and `new`: the lines a change adds or rewrites.
#[must_use]
pub fn changed_lines(old: &str, new: &str) -> Vec<u32> {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    // lengths[i][j]: the longest common sequence of old[i..] and new[j..].
    let mut lengths = vec![vec![0_usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lengths[i][j] = if old[i] == new[j] {
                lengths[i + 1][j + 1] + 1
            } else {
                lengths[i + 1][j].max(lengths[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut changed = Vec::new();
    while j < new.len() {
        if i < old.len() && old[i] == new[j] {
            i += 1;
            j += 1;
        } else if i < old.len() && lengths[i + 1][j] >= lengths[i][j + 1] {
            i += 1;
        } else {
            changed.push(u32::try_from(j + 1).unwrap_or(u32::MAX));
            j += 1;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_lines_marks_rewritten_and_added_lines_only() {
        assert_eq!(changed_lines("a\nb\nc\n", "a\nb\nc\n"), Vec::<u32>::new());
        assert_eq!(changed_lines("a\nb\nc\n", "a\nB\nc\n"), [2]);
        assert_eq!(changed_lines("a\nc\n", "a\nb\nc\nd\n"), [2, 4]);
        assert_eq!(changed_lines("a\nb\n", "b\n"), Vec::<u32>::new());
        assert_eq!(changed_lines("", "x\n"), [1]);
    }
}
