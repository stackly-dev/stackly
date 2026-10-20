//! `init`: prepares a data folder the way a real `initdb` would, and leaves
//! behind what a test needs to check the init plan.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::args::Flags;
use crate::{Failure, write_argv};

/// The argv record, the only entry `init` accepts in the data folder.
const ARGV_FILE: &str = "argv-init.json";
/// SHA-256 of the exact stdin bytes, never the token itself.
const TOKEN_DIGEST_FILE: &str = "token.sha256";
/// Data marker the `testing/fake` manifest names in `compatibility.dataMarker`.
const MARKER_FILE: &str = "FAKE_VERSION";

/// Runs `init --data=<dir> [--token-stdin] [--fail=<bool>] [--delay-ms=<n>]`.
pub(crate) fn run(argv: &[String], args: &[String]) -> Result<(), Failure> {
    let flags = Flags::parse(args, &["data", "fail", "delay-ms"], &["token-stdin"])?;
    let data = flags.required_path("data")?;
    let fail = flags.boolean("fail", false)?;
    let delay = Duration::from_millis(flags.number("delay-ms", 0)?);

    // Drain stdin before anything can fail, so the executor writing the
    // secret never meets a closed pipe.
    let token = if flags.switch("token-stdin") {
        Some(read_stdin()?)
    } else {
        None
    };

    // Hold the data folder untouched for the delay, so a test can stop the
    // daemon while the helper is still initializing.
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }

    ensure_private_dir(&data)?;
    write_argv(&data.join(ARGV_FILE), argv)?;
    reject_other_entries(&data)?;
    if fail {
        return Err(Failure::InitFailed);
    }
    if let Some(token) = token {
        let path = data.join(TOKEN_DIGEST_FILE);
        fs::write(&path, sha256_hex(&token))
            .map_err(|error| Failure::io("write", &path, &error))?;
    }
    let marker = data.join(MARKER_FILE);
    let line = format!(
        "{}.{}\n",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR")
    );
    fs::write(&marker, line).map_err(|error| Failure::io("write", &marker, &error))?;

    println!("initialized {}", data.display());
    Ok(())
}

/// Reads stdin until EOF, keeping every byte as it arrived.
fn read_stdin() -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .read_to_end(&mut bytes)
        .map_err(|error| Failure::Runtime(format!("cannot read the token from stdin: {error}")))?;
    Ok(bytes)
}

/// Creates the data folder with mode 0700 when it is missing. The init plan
/// normally created it already.
fn ensure_private_dir(data: &Path) -> Result<(), Failure> {
    match fs::metadata(data) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(Failure::Runtime(format!(
            "data path {} is not a folder",
            data.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(data)
            .map_err(|error| Failure::io("create", data, &error)),
        Err(error) => Err(Failure::io("inspect", data, &error)),
    }
}

/// Refuses a data folder that already holds anything but the argv record,
/// the way `initdb` refuses a folder that is not empty.
fn reject_other_entries(data: &Path) -> Result<(), Failure> {
    let entries = fs::read_dir(data).map_err(|error| Failure::io("read", data, &error))?;
    let mut others = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Failure::io("read", data, &error))?;
        let name = entry.file_name();
        if name != ARGV_FILE {
            others.push(name.to_string_lossy().into_owned());
        }
    }
    if others.is_empty() {
        return Ok(());
    }
    others.sort();
    Err(Failure::DataNotEmpty(format!(
        "data folder {} is not empty, it holds {}",
        data.display(),
        others.join(", ")
    )))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_lowercase_hex_of_the_exact_bytes() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(sha256_hex(b"abc\n"), sha256_hex(b"abc"));
    }
}
