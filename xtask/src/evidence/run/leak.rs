//! The fixture credential leak scan: `output.log`, every file under
//! `artifacts/`, and every file under `records/` must not hold
//! [`FIXTURE_CREDENTIAL_PREFIX`] (spec 0002, "Fixture credential").
//!
//! This is a best effort guard, not a guarantee: an encoded, cut, or
//! partly masked credential is not found. Symbolic links are never
//! followed, so nothing outside the run folder is scanned (a `TestRoot`
//! may hold a fixture credential on purpose).

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use stackly_evidence::{FIXTURE_CREDENTIAL_PREFIX, Problem, ProblemCode};
use uuid::Uuid;

/// How many bytes the scan reads from a file at a time.
const CHUNK: usize = 64 * 1024;

/// Scans the run folder and returns one `credential_leak` problem per file
/// that holds the prefix. A file or folder the scan cannot read is also a
/// `credential_leak` problem, because its absence cannot be shown.
pub fn scan(run_dir: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut files = vec![run_dir.join("output.log")];
    for folder in ["records", "artifacts"] {
        files_under(run_dir, &run_dir.join(folder), &mut files, &mut problems);
    }
    for file in files {
        let found = File::open(&file).and_then(holds_prefix);
        let message = match found {
            Ok(false) => continue,
            Ok(true) => format!(
                "fixture credential prefix found in {}",
                display(run_dir, &file)
            ),
            Err(err) => cannot_scan(run_dir, &file, &err),
        };
        problems.push(problem(run_dir, &file, message));
    }
    problems
}

/// Adds every regular file under `dir` to `files`, without following
/// symbolic links. A missing `dir` holds no files.
fn files_under(run_dir: &Path, dir: &Path, files: &mut Vec<PathBuf>, problems: &mut Vec<Problem>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return,
        Err(err) => {
            problems.push(problem(run_dir, dir, cannot_scan(run_dir, dir, &err)));
            return;
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        match entry.and_then(|entry| Ok((entry.path(), entry.file_type()?))) {
            Ok((path, file_type)) if file_type.is_dir() => {
                files_under(run_dir, &path, files, problems);
            }
            Ok((path, file_type)) if file_type.is_file() => found.push(path),
            // Symbolic links, sockets, and pipes are not scanned.
            Ok(_) => {}
            Err(err) => problems.push(problem(run_dir, dir, cannot_scan(run_dir, dir, &err))),
        }
    }
    found.sort();
    files.extend(found);
}

/// Reads `reader` to the end in chunks and reports whether the prefix
/// appears, including across two chunks.
fn holds_prefix(mut reader: impl Read) -> io::Result<bool> {
    let needle = FIXTURE_CREDENTIAL_PREFIX.as_bytes();
    // The tail kept between reads: long enough to hold all but the last
    // byte of a match that a read boundary splits.
    let keep = needle.len() - 1;
    let mut buffer = vec![0_u8; keep + CHUNK];
    let mut filled = 0;
    loop {
        let read = match reader.read(&mut buffer[filled..]) {
            Ok(0) => return Ok(false),
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        filled += read;
        if buffer[..filled]
            .windows(needle.len())
            .any(|window| window == needle)
        {
            return Ok(true);
        }
        let tail = filled.saturating_sub(keep);
        buffer.copy_within(tail..filled, 0);
        filled -= tail;
    }
}

fn problem(run_dir: &Path, path: &Path, message: String) -> Problem {
    Problem {
        code: ProblemCode::CredentialLeak,
        record_id: record_id_of(path.strip_prefix(run_dir).unwrap_or(path)),
        message,
    }
}

fn cannot_scan(run_dir: &Path, path: &Path, err: &io::Error) -> String {
    format!(
        "cannot scan {} for the fixture credential prefix: {err}",
        display(run_dir, path)
    )
}

/// The record a run folder path belongs to: `records/<id>.json` (also a
/// `<id>.json.tmp` that was never renamed) and everything under
/// `artifacts/<id>/`. Anything else is run level.
fn record_id_of(relative: &Path) -> Option<Uuid> {
    let mut parts = relative.components().filter_map(|part| match part {
        Component::Normal(name) => name.to_str(),
        _ => None,
    });
    let name = match (parts.next(), parts.next()) {
        (Some("records"), Some(file)) => file.split('.').next()?,
        (Some("artifacts"), Some(folder)) => folder,
        _ => return None,
    };
    name.parse().ok()
}

/// `path` relative to the run folder, with `/` as the separator.
fn display(run_dir: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(run_dir).unwrap_or(path);
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use stackly_evidence::FixtureCredential;

    use super::*;

    /// A run folder as the wrapper leaves it, with clean content.
    fn run_folder() -> (tempfile::TempDir, Uuid) {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::now_v7();
        fs::create_dir_all(dir.path().join("artifacts").join(id.to_string())).unwrap();
        fs::create_dir(dir.path().join("records")).unwrap();
        fs::write(dir.path().join("output.log"), "test tests::a ... ok\n").unwrap();
        fs::write(
            dir.path().join("records").join(format!("{id}.json")),
            r#"{"status":"PASS","actual":"connected as stackly"}"#,
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("artifacts")
                .join(id.to_string())
                .join("server.log"),
            "ready to accept connections\n",
        )
        .unwrap();
        (dir, id)
    }

    /// A reader that hands out at most `step` bytes per read.
    struct Trickle<'a> {
        data: &'a [u8],
        step: usize,
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let len = self.step.min(buf.len()).min(self.data.len());
            buf[..len].copy_from_slice(&self.data[..len]);
            self.data = &self.data[len..];
            Ok(len)
        }
    }

    #[test]
    fn a_clean_run_folder_has_no_problems() {
        let (dir, _) = run_folder();
        assert!(scan(dir.path()).is_empty());
    }

    #[test]
    fn a_credential_in_output_log_is_a_run_level_leak() {
        let (dir, _) = run_folder();
        let credential = FixtureCredential::new();
        fs::write(
            dir.path().join("output.log"),
            format!("connecting with password {}\n", credential.value()),
        )
        .unwrap();

        let problems = scan(dir.path());
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, ProblemCode::CredentialLeak);
        assert_eq!(problems[0].record_id, None);
        assert_eq!(
            problems[0].message,
            "fixture credential prefix found in output.log"
        );
    }

    #[test]
    fn a_credential_in_a_record_is_a_leak_of_that_record() {
        let (dir, id) = run_folder();
        let credential = FixtureCredential::new();
        fs::write(
            dir.path().join("records").join(format!("{id}.json")),
            format!(
                r#"{{"status":"PASS","actual":"logged in with {}"}}"#,
                credential.value()
            ),
        )
        .unwrap();

        let problems = scan(dir.path());
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, ProblemCode::CredentialLeak);
        assert_eq!(problems[0].record_id, Some(id));
        assert_eq!(
            problems[0].message,
            format!("fixture credential prefix found in records/{id}.json")
        );
    }

    #[test]
    fn a_credential_in_a_nested_artifact_is_a_leak_of_its_record() {
        let (dir, id) = run_folder();
        let nested = dir
            .path()
            .join("artifacts")
            .join(id.to_string())
            .join("logs");
        fs::create_dir(&nested).unwrap();
        let credential = FixtureCredential::new();
        fs::write(nested.join("daemon.log"), credential.value()).unwrap();

        let problems = scan(dir.path());
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].record_id, Some(id));
        assert_eq!(
            problems[0].message,
            format!("fixture credential prefix found in artifacts/{id}/logs/daemon.log")
        );
    }

    #[test]
    fn a_prefix_split_across_two_reads_is_found() {
        let credential = FixtureCredential::new();
        let mut data = vec![b'x'; CHUNK - 5];
        data.extend_from_slice(credential.value().as_bytes());
        data.extend_from_slice(b"\ntrailing output\n");

        assert!(holds_prefix(data.as_slice()).unwrap());
        assert!(
            holds_prefix(Trickle {
                data: &data,
                step: 3
            })
            .unwrap()
        );
        assert!(
            !holds_prefix(Trickle {
                data: &vec![b'x'; CHUNK * 2],
                step: 4096
            })
            .unwrap()
        );
    }

    #[test]
    fn a_cut_prefix_is_not_a_leak() {
        let cut = &FIXTURE_CREDENTIAL_PREFIX[..FIXTURE_CREDENTIAL_PREFIX.len() - 1];
        assert!(!holds_prefix(cut.as_bytes()).unwrap());
    }

    #[test]
    fn a_missing_output_log_fails_the_scan() {
        let (dir, _) = run_folder();
        fs::remove_file(dir.path().join("output.log")).unwrap();

        let problems = scan(dir.path());
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, ProblemCode::CredentialLeak);
        assert!(
            problems[0]
                .message
                .starts_with("cannot scan output.log for the fixture credential prefix: ")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_out_of_the_run_folder_is_not_followed() {
        let (dir, id) = run_folder();
        let test_root = tempfile::tempdir().unwrap();
        let credential = FixtureCredential::new();
        let secret = test_root.path().join("password.txt");
        fs::write(&secret, credential.value()).unwrap();
        std::os::unix::fs::symlink(
            &secret,
            dir.path()
                .join("artifacts")
                .join(id.to_string())
                .join("password.txt"),
        )
        .unwrap();
        std::os::unix::fs::symlink(test_root.path(), dir.path().join("artifacts").join("root"))
            .unwrap();

        assert!(scan(dir.path()).is_empty());
    }

    #[test]
    fn record_ids_come_from_records_and_artifact_folders() {
        let id = Uuid::now_v7();
        assert_eq!(
            record_id_of(Path::new(&format!("records/{id}.json"))),
            Some(id)
        );
        assert_eq!(
            record_id_of(Path::new(&format!("records/{id}.json.tmp"))),
            Some(id)
        );
        assert_eq!(
            record_id_of(Path::new(&format!("artifacts/{id}/a/b.log"))),
            Some(id)
        );
        assert_eq!(record_id_of(Path::new("artifacts/notes.txt")), None);
        assert_eq!(record_id_of(Path::new("output.log")), None);
    }
}
