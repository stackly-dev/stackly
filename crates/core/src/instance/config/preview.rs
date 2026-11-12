//! The Effective File of the Configuration page (spec 0029, *Evaluator
//! config*): the non sensitive config files a set of values renders,
//! without resolving a secret, and the lines a proposal changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use stackly_domain::{ConfigRevisionId, Endpoint, Instance, Port, PortRequest};
use stackly_platform::{FileKind, Filesystem, InstancePaths, PlatformError};

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
                Segment::Secret { .. } | Segment::SecretHash { .. } => None,
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

/// Largest history copy the page reads.
const MAX_COPY: u64 = 16 * 1024 * 1024;
/// Deepest folder nesting below a history folder.
const MAX_COPY_DEPTH: usize = 8;

/// The copies of the last render of `revision`, as the page shows them
/// (spec 0029, AC-1, AC-10): every file in `config-history/<revision>/`,
/// ordered by path. A revision that was never rendered has none.
///
/// # Errors
///
/// Returns the [`PlatformError`] of a folder or file that cannot be read.
pub fn recorded_files(
    fs: &dyn Filesystem,
    paths: &InstancePaths,
    revision: ConfigRevisionId,
) -> Result<Vec<EffectiveFile>, PlatformError> {
    let history = crate::supervisor::drift::history_dir(paths, revision);
    let mut relatives = Vec::new();
    if fs
        .symlink_metadata(&history)?
        .is_some_and(|info| info.kind == FileKind::Dir)
    {
        collect(fs, &history, Path::new(""), 0, &mut relatives)?;
    }
    let mut files = Vec::new();
    for relative in relatives {
        let bytes = fs.read_file_limited(&history.join(&relative), MAX_COPY)?;
        let Some(path) = relative.to_str() else {
            continue;
        };
        files.push(EffectiveFile {
            path: path.to_owned(),
            content: String::from_utf8_lossy(&bytes).into_owned(),
            changed_lines: Vec::new(),
            preview: false,
            port_note: None,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The regular files below `dir`, relative to the history folder.
fn collect(
    fs: &dyn Filesystem,
    dir: &Path,
    relative: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
) -> Result<(), PlatformError> {
    if depth > MAX_COPY_DEPTH {
        return Ok(());
    }
    for entry in fs.read_dir(&dir.join(relative))? {
        let name = relative.join(&entry.name);
        match entry.info.kind {
            FileKind::Dir => collect(fs, dir, &name, depth + 1, out)?,
            FileKind::File => out.push(name),
            _ => {}
        }
    }
    Ok(())
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
