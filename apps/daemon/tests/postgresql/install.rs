//! The exec check of the install pipeline on the real artifacts (spec 0019
//! AC-15, spec 0018): `bin/postgres --version` prints the version the
//! catalog names.

use std::process::Command;

use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{OWNER, PG17, PG18, Site, artifact_sha256, cached_artifact};

#[test]
#[ignore = "native: needs both managed PostgreSQL artifacts in the runtime cache"]
fn postgresql_17_and_18_pass_the_install_exec_check_with_the_version_of_their_artifact() {
    let ev = evidence!(
        covers = ["REQ-035"],
        kind = Kind::Native,
        expected = "the install pipeline installs both PostgreSQL artifacts with the exec check of runtime.versionCheck, and bin/postgres --version prints postgres (PostgreSQL) 17.11 and 18.6",
    );
    let missing: Vec<&str> = [PG17, PG18]
        .into_iter()
        .filter(|pg| cached_artifact(&artifact_sha256(*pg)).is_none())
        .map(|pg| pg.artifact)
        .collect();
    if !missing.is_empty() {
        ev.blocked(
            &format!(
                "artifact {} is not in the runtime cache",
                missing.join(", ")
            ),
            OWNER,
        );
        return;
    }
    let mut seen = Vec::new();
    for pg in [PG17, PG18] {
        let site = Site::open(&ev, pg).expect("the artifact is in the cache");
        let output = Command::new(site.bin("postgres"))
            .arg("--version")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("postgres runs");
        let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert_eq!(line, format!("postgres (PostgreSQL) {}", pg.version));
        site.subject(&ev, pg.version);
        seen.push(line);
    }
    ev.pass_with(&seen.join("; "));
}
