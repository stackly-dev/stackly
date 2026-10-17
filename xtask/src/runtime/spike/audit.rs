//! The library audits of the spike (spec 0003, AC-6 and AC-7).
//!
//! Static: every Mach-O file of the artifact must carry an arm64 slice,
//! and every library it references (`LC_LOAD_DYLIB` and friends) must be a
//! system library or a file inside the artifact, found through its
//! `@rpath`, `@loader_path`, or `@executable_path`. Runtime: every Mach-O
//! file the running server maps (`lsof` `txt` entries) must be a system
//! file or a file of the artifact.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::archive::walk;
use super::result::Listener;
use crate::runtime::macho::{FileKind, file_kind};

const OTOOL: &str = "/usr/bin/otool";
const LIPO: &str = "/usr/bin/lipo";
const LSOF: &str = "/usr/sbin/lsof";
const PGREP: &str = "/usr/bin/pgrep";

/// Install name prefixes of system libraries, for the static audit.
const SYSTEM_STATIC: [&str; 2] = ["/usr/lib/", "/System/Library/"];
/// Folders a running server may map Mach-O files from, besides the
/// artifact (AC-6).
const SYSTEM_RUNTIME: [&str; 4] = [
    "/usr/lib/",
    "/System/Library/",
    "/System/Volumes/Preboot/Cryptexes/OS/System/Library/",
    "/System/Cryptexes/OS/System/Library/",
];
/// `MH_EXECUTE` in the Mach-O header.
const MH_EXECUTE: u32 = 2;

/// One library reference of a Mach-O file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRef {
    /// The install name as `otool` prints it.
    pub name: String,
    /// True for `LC_LOAD_WEAK_DYLIB`: missing is allowed.
    pub weak: bool,
}

/// What `otool -h -l` says about one arm64 slice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadCommands {
    /// The Mach-O file type, such as 2 for an executable.
    pub filetype: u32,
    /// `LC_ID_DYLIB`, for a dylib.
    pub id: Option<String>,
    /// Every library it loads.
    pub libraries: Vec<LibraryRef>,
    /// Every `LC_RPATH`.
    pub rpaths: Vec<String>,
}

/// One Mach-O file of the artifact.
#[derive(Debug, Clone)]
pub struct MachO {
    /// Relative to the artifact root.
    pub relative: String,
    /// The absolute path.
    pub path: PathBuf,
    /// Its arm64 load commands.
    pub commands: LoadCommands,
}

impl MachO {
    fn is_executable(&self) -> bool {
        self.commands.filetype == MH_EXECUTE
    }

    fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("/"))
    }
}

/// Finds every Mach-O file under `root` by its magic number and reads the
/// load commands of its arm64 slice. A file without one fails the audit.
pub fn scan(root: &Path) -> Result<Vec<MachO>> {
    let mut found = Vec::new();
    for entry in walk(root)? {
        if !entry.meta.is_file() || file_kind(&entry.path)? != FileKind::MachO {
            continue;
        }
        let archs = command_text(Command::new(LIPO).arg("-archs").arg(&entry.path))?;
        if !archs.split_whitespace().any(|arch| arch == "arm64") {
            bail!(
                "{} has no arm64 slice (it has {})",
                entry.relative,
                archs.trim()
            );
        }
        let text = command_text(
            Command::new(OTOOL)
                .args(["-arch", "arm64", "-h", "-l"])
                .arg(&entry.path),
        )?;
        found.push(MachO {
            relative: entry.relative,
            path: entry.path,
            commands: parse_load_commands(&text),
        });
    }
    Ok(found)
}

fn command_text(command: &mut Command) -> Result<String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command
        .output()
        .with_context(|| format!("cannot run {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Reads `otool -h -l` output: the file type from the header, and the
/// dylib, id, and rpath load commands.
pub fn parse_load_commands(text: &str) -> LoadCommands {
    let mut parsed = LoadCommands::default();
    let mut lines = text.lines().peekable();
    let mut command = String::new();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("magic ") || trimmed == "magic" {
            if let Some(header) = lines.peek() {
                parsed.filetype = header
                    .split_whitespace()
                    .nth(4)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
            continue;
        }
        if let Some(cmd) = trimmed.strip_prefix("cmd ") {
            command = cmd.trim().to_owned();
            continue;
        }
        let value = |key: &str| {
            trimmed
                .strip_prefix(key)
                .map(|rest| rest.trim_start())
                .map(|rest| {
                    rest.rsplit_once(" (offset ")
                        .map_or(rest, |(path, _)| path)
                        .to_owned()
                })
        };
        match command.as_str() {
            "LC_LOAD_DYLIB"
            | "LC_REEXPORT_DYLIB"
            | "LC_LAZY_LOAD_DYLIB"
            | "LC_LOAD_UPWARD_DYLIB"
            | "LC_LOAD_WEAK_DYLIB" => {
                if let Some(name) = value("name ") {
                    parsed.libraries.push(LibraryRef {
                        name,
                        weak: command == "LC_LOAD_WEAK_DYLIB",
                    });
                }
            }
            "LC_ID_DYLIB" => {
                if let Some(name) = value("name ") {
                    parsed.id = Some(name);
                }
            }
            "LC_RPATH" => {
                if let Some(path) = value("path ") {
                    parsed.rpaths.push(path);
                }
            }
            _ => {}
        }
    }
    parsed
}

/// The outcome of the static audit.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StaticAudit {
    /// Every resolved library: system install names as they are, files of
    /// the artifact relative to its root.
    pub libraries: BTreeSet<String>,
    /// File names of the non system libraries found (AC-7).
    pub non_system: BTreeSet<String>,
    /// Paths outside the allowlist and the artifact.
    pub foreign: Vec<String>,
    /// Findings that do not fail the spike.
    pub warnings: Vec<String>,
}

/// Resolves every library reference of every Mach-O file. An absolute
/// `LC_RPATH` outside the system is foreign for a maintainer artifact and
/// a warning for an upstream one.
pub fn static_audit(root: &Path, machos: &[MachO], maintainer: bool) -> Result<StaticAudit> {
    let root = root
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", root.display()))?;
    let executables: Vec<&MachO> = machos.iter().filter(|m| m.is_executable()).collect();
    let mut audit = StaticAudit::default();
    for macho in machos {
        for rpath in &macho.commands.rpaths {
            if rpath.starts_with('/') && !under(rpath, &SYSTEM_STATIC) {
                let finding = format!(
                    "{}: LC_RPATH {rpath} is absolute outside the system",
                    macho.relative
                );
                if maintainer {
                    audit.foreign.push(finding);
                } else {
                    audit.warnings.push(finding);
                }
            }
        }
        for library in &macho.commands.libraries {
            if macho.commands.id.as_deref() == Some(library.name.as_str()) {
                continue;
            }
            if under(&library.name, &SYSTEM_STATIC) {
                audit.libraries.insert(library.name.clone());
                continue;
            }
            match resolve(&root, macho, &library.name, &executables) {
                Resolved::Inside(relative) => {
                    let file_name = library
                        .name
                        .rsplit('/')
                        .next()
                        .unwrap_or(&library.name)
                        .to_owned();
                    audit.non_system.insert(file_name);
                    audit.libraries.insert(relative);
                }
                Resolved::System(path) => {
                    audit.libraries.insert(path);
                }
                Resolved::Outside(path) => audit
                    .foreign
                    .push(format!("{path} (loaded by {})", macho.relative)),
                Resolved::Missing if library.weak => audit.warnings.push(format!(
                    "{}: weak library {} is missing",
                    macho.relative, library.name
                )),
                Resolved::Missing => audit.foreign.push(format!(
                    "{} (loaded by {}, not found)",
                    library.name, macho.relative
                )),
            }
        }
    }
    audit.foreign.sort();
    audit.foreign.dedup();
    audit.warnings.sort();
    audit.warnings.dedup();
    Ok(audit)
}

enum Resolved {
    /// A file inside the artifact, relative to its root.
    Inside(String),
    /// A system file reached through a relative reference.
    System(String),
    /// An existing file outside the artifact and the system.
    Outside(String),
    /// Nothing found.
    Missing,
}

/// Resolves one reference the way dyld would, trying the file's own
/// rpaths first and then those of every executable of the artifact.
fn resolve(root: &Path, macho: &MachO, name: &str, executables: &[&MachO]) -> Resolved {
    let exe_dirs: Vec<&Path> = if macho.is_executable() {
        vec![macho.dir()]
    } else {
        executables.iter().map(|exe| exe.dir()).collect()
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(rest) = name.strip_prefix("@rpath/") {
        let mut rpaths: Vec<PathBuf> = Vec::new();
        for rpath in &macho.commands.rpaths {
            rpaths.extend(expand(rpath, macho.dir(), &exe_dirs));
        }
        for exe in executables {
            for rpath in &exe.commands.rpaths {
                rpaths.extend(expand(rpath, exe.dir(), &[exe.dir()]));
            }
        }
        candidates.extend(rpaths.into_iter().map(|dir| dir.join(rest)));
    } else if name.starts_with('@') {
        candidates.extend(expand(name, macho.dir(), &exe_dirs));
    } else {
        candidates.push(PathBuf::from(name));
    }
    for candidate in candidates {
        let Ok(real) = candidate.canonicalize() else {
            continue;
        };
        if !real.is_file() {
            continue;
        }
        return match real.strip_prefix(root) {
            Ok(relative) => Resolved::Inside(relative.to_string_lossy().into_owned()),
            Err(_) if under(&real.to_string_lossy(), &SYSTEM_RUNTIME) => {
                Resolved::System(real.to_string_lossy().into_owned())
            }
            Err(_) => Resolved::Outside(real.to_string_lossy().into_owned()),
        };
    }
    Resolved::Missing
}

/// Expands `@loader_path` and `@executable_path` in a path. An
/// `@executable_path` path gives one candidate per executable folder.
fn expand(path: &str, loader_dir: &Path, exe_dirs: &[&Path]) -> Vec<PathBuf> {
    if let Some(rest) = path.strip_prefix("@loader_path") {
        vec![join_rest(loader_dir, rest)]
    } else if let Some(rest) = path.strip_prefix("@executable_path") {
        exe_dirs.iter().map(|dir| join_rest(dir, rest)).collect()
    } else if path.starts_with('@') {
        Vec::new()
    } else {
        vec![PathBuf::from(path)]
    }
}

fn join_rest(dir: &Path, rest: &str) -> PathBuf {
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(rest)
    }
}

fn under(path: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| path.starts_with(prefix))
        && !path.split('/').any(|part| part == "..")
}

/// The Mach-O files the running processes `pids` map: system files as
/// absolute paths, artifact files relative to `root`, and the foreign
/// ones, both sorted.
pub fn runtime_libraries(pids: &[u32], root: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let root = root
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", root.display()))?;
    let pids = pid_list(pids);
    let output = Command::new(LSOF)
        .env("LANG", "en_US.UTF-8")
        .args(["-nP", "-p", &pids, "-Ffn"])
        .output()
        .with_context(|| format!("cannot run {LSOF}"))?;
    if !output.status.success() {
        bail!(
            "lsof cannot read processes {pids}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut runtime = BTreeSet::new();
    let mut foreign = BTreeSet::new();
    for name in parse_lsof_txt(&String::from_utf8_lossy(&output.stdout)) {
        let path = PathBuf::from(&name);
        if !path.is_file() || file_kind(&path).ok() != Some(FileKind::MachO) {
            continue;
        }
        let real = path.canonicalize().unwrap_or(path);
        let text = real.to_string_lossy().into_owned();
        if let Ok(relative) = real.strip_prefix(&root) {
            runtime.insert(relative.to_string_lossy().into_owned());
        } else if under(&text, &SYSTEM_RUNTIME) {
            runtime.insert(text);
        } else {
            runtime.insert(text.clone());
            foreign.insert(text);
        }
    }
    Ok((runtime.into_iter().collect(), foreign.into_iter().collect()))
}

/// The names of the `txt` entries in `lsof -Ffn` output, unescaped.
pub fn parse_lsof_txt(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut fd = "";
    for line in text.lines() {
        if let Some(value) = line.strip_prefix('f') {
            fd = value;
        } else if let Some(name) = line.strip_prefix('n') {
            if fd == "txt" {
                names.push(unescape(name));
            }
        } else if line.starts_with('p') {
            fd = "";
        }
    }
    names
}

/// Undoes lsof's `\xNN` escapes of bytes it found unprintable.
fn unescape(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1] == b'x'
            && let Some(byte) = std::str::from_utf8(&bytes[i + 2..i + 4])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Every TCP listener of `pids`, from `lsof -iTCP -sTCP:LISTEN`.
pub fn listeners(pids: &[u32]) -> Result<Vec<Listener>> {
    let output = Command::new(LSOF)
        .args([
            "-nP",
            "-a",
            "-p",
            &pid_list(pids),
            "-iTCP",
            "-sTCP:LISTEN",
            "-Fn",
        ])
        .output()
        .with_context(|| format!("cannot run {LSOF}"))?;
    // lsof exits 1 when the process has no matching file.
    Ok(parse_lsof_listen(&String::from_utf8_lossy(&output.stdout)))
}

/// Every live process in the process group `pgid`, the leader first.
///
/// A server may fork its real work into children, such as the watchdog of
/// ClickHouse or the backends of PostgreSQL; the runtime audit covers
/// them all.
pub fn group_pids(pgid: u32) -> Result<Vec<u32>> {
    let output = Command::new(PGREP)
        .args(["-g", &pgid.to_string()])
        .output()
        .with_context(|| format!("cannot run {PGREP}"))?;
    // pgrep exits 1 when nothing matches; the leader is listed anyway.
    let mut pids = vec![pgid];
    pids.extend(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .filter(|pid| *pid != pgid),
    );
    Ok(pids)
}

fn pid_list(pids: &[u32]) -> String {
    pids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Reads `n<address>:<port>` lines.
pub fn parse_lsof_listen(text: &str) -> Vec<Listener> {
    let mut found: Vec<Listener> = text
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .filter_map(|name| {
            let (address, port) = name.rsplit_once(':')?;
            Some(Listener {
                address: address.to_owned(),
                port: port.parse().ok()?,
            })
        })
        .collect();
    found.sort_by(|a, b| (a.port, &a.address).cmp(&(b.port, &b.address)));
    found.dedup();
    found
}

/// True for a loopback listener address.
pub fn is_loopback(address: &str) -> bool {
    matches!(address, "127.0.0.1" | "[::1]" | "localhost") || address.starts_with("127.")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MYSQLD: &str = "\
/x/bin/mysqld:
Mach header
      magic  cputype cpusubtype  caps    filetype ncmds sizeofcmds      flags
 0xfeedfacf 16777228          0  0x00           2    30       4000 0x00a18085
Load command 12
          cmd LC_LOAD_DYLIB
      cmdsize 56
         name @rpath/libssl.3.dylib (offset 24)
   time stamp 2 Thu Jan  1 07:00:02 1970
Load command 13
          cmd LC_LOAD_WEAK_DYLIB
      cmdsize 56
         name /usr/lib/libSystem.B.dylib (offset 24)
Load command 20
          cmd LC_RPATH
      cmdsize 40
         path @loader_path/../lib/ (offset 12)
Load command 21
          cmd LC_RPATH
      cmdsize 40
         path /usr/local/mysql/lib (offset 12)
";

    #[test]
    fn load_commands_give_type_libraries_and_rpaths() {
        let parsed = parse_load_commands(MYSQLD);
        assert_eq!(parsed.filetype, MH_EXECUTE);
        assert_eq!(parsed.id, None);
        assert_eq!(
            parsed.libraries,
            [
                LibraryRef {
                    name: "@rpath/libssl.3.dylib".to_owned(),
                    weak: false
                },
                LibraryRef {
                    name: "/usr/lib/libSystem.B.dylib".to_owned(),
                    weak: true
                },
            ]
        );
        assert_eq!(
            parsed.rpaths,
            ["@loader_path/../lib/", "/usr/local/mysql/lib"]
        );
    }

    fn macho(root: &Path, relative: &str, text: &str) -> MachO {
        MachO {
            relative: relative.to_owned(),
            path: root.join(relative),
            commands: parse_load_commands(text),
        }
    }

    #[test]
    fn rpath_libraries_inside_the_artifact_pass_and_others_are_foreign() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("bin/mysqld"), "").unwrap();
        std::fs::write(root.join("lib/libssl.3.dylib"), "").unwrap();
        let exe = macho(root, "bin/mysqld", MYSQLD);
        let upstream = static_audit(root, std::slice::from_ref(&exe), false).unwrap();
        assert_eq!(
            upstream.non_system,
            BTreeSet::from(["libssl.3.dylib".to_owned()])
        );
        assert!(upstream.libraries.contains("lib/libssl.3.dylib"));
        assert!(upstream.libraries.contains("/usr/lib/libSystem.B.dylib"));
        assert!(upstream.foreign.is_empty(), "{:?}", upstream.foreign);
        assert_eq!(
            upstream.warnings,
            ["bin/mysqld: LC_RPATH /usr/local/mysql/lib is absolute outside the system"]
        );

        let maintainer = static_audit(root, std::slice::from_ref(&exe), true).unwrap();
        assert_eq!(
            maintainer.foreign,
            ["bin/mysqld: LC_RPATH /usr/local/mysql/lib is absolute outside the system"]
        );

        let homebrew = MYSQLD.replace("@rpath/libssl.3.dylib", "/opt/homebrew/lib/libfoo.dylib");
        let exe = macho(root, "bin/mysqld", &homebrew);
        let audit = static_audit(root, &[exe], false).unwrap();
        assert_eq!(
            audit.foreign,
            ["/opt/homebrew/lib/libfoo.dylib (loaded by bin/mysqld, not found)"]
        );
    }

    #[test]
    fn lsof_txt_names_are_unescaped_and_other_fds_skipped() {
        let text = "p42\nfcwd\nn/x\nftxt\nn/tmp/Stackly Spike \\xc3\\x9c/bin/nats-server\nftxt\nn/usr/lib/dyld\nf3\nn/dev/null\n";
        assert_eq!(
            parse_lsof_txt(text),
            ["/tmp/Stackly Spike Ü/bin/nats-server", "/usr/lib/dyld"]
        );
    }

    #[test]
    fn listeners_are_read_and_loopback_is_recognised() {
        let text = "p42\nf5\nn127.0.0.1:4222\nf6\nn*:8222\nf7\nn[::1]:9000\n";
        let found = parse_lsof_listen(text);
        assert_eq!(found.len(), 3);
        assert_eq!(
            found[0],
            Listener {
                address: "127.0.0.1".to_owned(),
                port: 4222
            }
        );
        assert!(is_loopback("127.0.0.1") && is_loopback("[::1]"));
        assert!(!is_loopback("*") && !is_loopback("0.0.0.0"));
    }
}
