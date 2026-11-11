//! The process contract of spec 0036 (*Kontrak proses*, AC-10), the answer
//! to the Follow-up of spec 0010: two samples of the main process two
//! seconds apart inside the `process` step, and the `leftovers` check
//! after a stop. Files are matched by device and inode, never by path
//! text, because `lsof` rewrites non ASCII bytes. Every holder search
//! leaves out this process, since the daemon runs in it.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use uuid::Uuid;

use super::steps::{Ctx, Outcome, get};
use super::{Failure, detail};

/// The gap between the two samples.
pub const SAMPLE_GAP: Duration = Duration::from_secs(2);
/// How long `leftovers` waits for a process or a port to go away.
const SETTLE: Duration = Duration::from_secs(3);

/// A device and inode pair.
pub type FileId = (u64, u64);

/// One sample of the main process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    /// The PID `instance.get` reports.
    pub pid: u32,
    /// `startedAt` of `instance.get`.
    pub started_at: String,
    /// `process.adoptable` of `instance.get`.
    pub adoptable: bool,
    /// The file at fd 3, from `lsof -a -p <pid> -d 3 -F Di`.
    pub fd3: Option<FileId>,
    /// The first `txt` entry, the executable, from `lsof -d txt`.
    pub txt: Option<FileId>,
    /// `ps -o pgid= -p <pid>`.
    pub pgid: Option<u32>,
}

/// The device and inode of `path`, without following a symlink.
#[must_use]
pub fn file_id(path: &Path) -> Option<FileId> {
    std::fs::symlink_metadata(path)
        .ok()
        .map(|meta| (meta.dev(), meta.ino()))
}

/// The device and inode `lsof -F Di` reports for the first descriptor of
/// `pid` that `descriptor` selects (`3` or `txt`).
#[must_use]
pub fn lsof_file(pid: u32, descriptor: &str) -> Option<FileId> {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", descriptor, "-F", "Di"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    parse_lsof(&String::from_utf8_lossy(&output.stdout), descriptor)
}

/// The first `D` and `i` pair after an `f<descriptor>` line of `lsof -F`.
#[must_use]
pub fn parse_lsof(text: &str, descriptor: &str) -> Option<FileId> {
    let mut inside = false;
    let mut device = None;
    for line in text.lines() {
        let (tag, value) = line.split_at_checked(1)?;
        match tag {
            "f" => {
                if inside && device.is_some() {
                    return None;
                }
                inside = value == descriptor;
                device = None;
            }
            "D" if inside => {
                device = u64::from_str_radix(value.trim_start_matches("0x"), 16).ok();
            }
            "i" if inside => {
                let inode = value.parse().ok()?;
                return device.map(|device| (device, inode));
            }
            _ => {}
        }
    }
    None
}

/// The process group of `pid`.
#[must_use]
pub fn pgid(pid: u32) -> Option<u32> {
    let output = Command::new("/bin/ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// The PIDs other than this process whose open files include `path`.
fn lock_holders(path: &Path) -> Vec<u32> {
    let own = std::process::id();
    Command::new("/usr/sbin/lsof")
        .arg("-t")
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
                .filter(|pid| *pid != own)
                .collect()
        })
        .unwrap_or_default()
}

/// The PIDs in process group `pgid`, other than this process.
fn group_members(pgid: u32) -> Vec<u32> {
    let own = std::process::id();
    Command::new("/usr/bin/pgrep")
        .args(["-g", &pgid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
                .filter(|pid| *pid != own)
                .collect()
        })
        .unwrap_or_default()
}

/// The program name of `pid`, without its folder or arguments, from
/// `ps -o comm=`; empty when the process is gone.
#[must_use]
pub fn command_name(pid: u32) -> String {
    Command::new("/bin/ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .and_then(|path| {
            Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

/// `pid` and the program name without arguments, for the report.
fn described(pids: &[u32]) -> Value {
    let rows: Vec<Value> = pids
        .iter()
        .map(|pid| json!({ "pid": pid, "command": command_name(*pid) }))
        .collect();
    Value::Array(rows)
}

/// Takes one sample of instance `id` through `instance.get`, `lsof`, and
/// `ps`.
async fn sample(ctx: &Ctx, id: Uuid) -> Result<Sample, Outcome> {
    let summary = get(ctx, id)
        .await
        .map_err(|error| Outcome::Fail(Failure::error(error.code(), error.message()), None))?;
    let process = &summary["process"];
    let Some(pid) = process["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
    else {
        return Err(Outcome::Fail(
            Failure::reason("not_foreground", "instance.get shows no main process"),
            None,
        ));
    };
    Ok(Sample {
        pid,
        started_at: process["startedAt"].as_str().unwrap_or_default().to_owned(),
        adoptable: process["adoptable"] == true,
        fd3: lsof_file(pid, "3"),
        txt: lsof_file(pid, "txt"),
        pgid: pgid(pid),
    })
}

/// The `process` step: two samples [`SAMPLE_GAP`] apart, compared by
/// [`compare`]. `first` gets the first sample.
pub async fn check(
    ctx: &Ctx,
    id: Uuid,
    program: Option<PathBuf>,
    first: &mut Option<Sample>,
) -> Outcome {
    let lock = ctx
        .layout
        .layout()
        .instance_paths(
            id.to_string()
                .parse()
                .expect("the daemon hands out instance IDs"),
        )
        .lock;
    let one = match sample(ctx, id).await {
        Ok(sample) => sample,
        Err(outcome) => return outcome,
    };
    *first = Some(one.clone());
    tokio::time::sleep(SAMPLE_GAP).await;
    let two = match sample(ctx, id).await {
        Ok(sample) => sample,
        Err(outcome) => return outcome,
    };
    let program_id = program.as_deref().and_then(file_id);
    match compare(&one, &two, file_id(&lock), program_id) {
        Ok(()) => Outcome::Pass(detail(json!({ "pid": one.pid, "samples": 2 }))),
        Err(failure) => Outcome::Fail(failure, detail(json!({ "pid": one.pid }))),
    }
}

/// The three checks of *Kontrak proses* on two samples: fd 3 is the
/// instance lock (`lock_not_held`), the executable is the runtime program
/// in both (`re_exec`), and the process stays the same group leader
/// (`not_foreground`).
///
/// # Errors
///
/// The failure of the first check that does not hold.
pub fn compare(
    one: &Sample,
    two: &Sample,
    lock: Option<FileId>,
    program: Option<FileId>,
) -> Result<(), Failure> {
    for sample in [one, two] {
        if !sample.adoptable || lock.is_none() || sample.fd3 != lock {
            return Err(Failure::reason(
                "lock_not_held",
                format!(
                    "pid {}: adoptable {}, fd 3 {:?}, instance.lock {lock:?}",
                    sample.pid, sample.adoptable, sample.fd3
                ),
            ));
        }
    }
    for sample in [one, two] {
        if program.is_none() || sample.txt != program {
            return Err(Failure::reason(
                "re_exec",
                format!(
                    "pid {}: executable {:?}, runtime program {program:?}",
                    sample.pid, sample.txt
                ),
            ));
        }
    }
    if one.pid != two.pid || one.started_at != two.started_at {
        return Err(Failure::reason(
            "not_foreground",
            format!(
                "the main process changed from {} ({}) to {} ({})",
                one.pid, one.started_at, two.pid, two.started_at
            ),
        ));
    }
    for sample in [one, two] {
        if sample.pgid != Some(sample.pid) {
            return Err(Failure::reason(
                "not_foreground",
                format!(
                    "pid {} is not the leader of its process group {:?}",
                    sample.pid, sample.pgid
                ),
            ));
        }
    }
    Ok(())
}

/// The `leftovers` check after a stop: no process other than this one
/// holds `lock` (`lock_still_held`), the process group `pgid` is empty
/// (`process_left`), and no tcp `port` accepts a connection
/// (`port_still_open`). Without a known group, the group of a process
/// still holding the lock is checked. It waits up to three seconds for
/// each to settle.
#[must_use]
pub fn leftovers(lock: &Path, pgid: Option<u32>, ports: &[u16]) -> Outcome {
    let deadline = Instant::now() + SETTLE;
    loop {
        let holders = lock_holders(lock);
        let group = pgid.or_else(|| holders.iter().find_map(|pid| self::pgid(*pid)));
        let members = group.map(group_members).unwrap_or_default();
        let open: Vec<u16> = ports
            .iter()
            .copied()
            .filter(|port| {
                TcpStream::connect_timeout(
                    &SocketAddr::from((Ipv4Addr::LOCALHOST, *port)),
                    Duration::from_millis(200),
                )
                .is_ok()
            })
            .collect();
        let clean = holders.is_empty() && members.is_empty() && open.is_empty();
        if clean {
            return Outcome::Pass(detail(json!({ "ports": ports.len(), "group": group })));
        }
        if Instant::now() >= deadline {
            let mut reasons = Vec::new();
            if !holders.is_empty() {
                reasons.push("lock_still_held");
            }
            if !members.is_empty() {
                reasons.push("process_left");
            }
            if !open.is_empty() {
                reasons.push("port_still_open");
            }
            let mut pids = holders.clone();
            for pid in &members {
                if !pids.contains(pid) {
                    pids.push(*pid);
                }
            }
            return Outcome::Fail(
                Failure::reason(reasons[0], format!("after stop: {}", reasons.join(", "))),
                detail(json!({
                    "reasons": reasons,
                    "processes": described(&pids),
                    "openPorts": open,
                })),
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(pid: u32, started_at: &str, pgid: u32) -> Sample {
        Sample {
            pid,
            started_at: started_at.to_owned(),
            adoptable: true,
            fd3: Some((1, 10)),
            txt: Some((1, 20)),
            pgid: Some(pgid),
        }
    }

    const LOCK: Option<FileId> = Some((1, 10));
    const PROGRAM: Option<FileId> = Some((1, 20));

    fn reason(result: Result<(), Failure>) -> String {
        result.unwrap_err().reason.unwrap()
    }

    #[test]
    fn two_equal_samples_of_a_group_leader_hold_the_contract() {
        let one = sample(41, "t1", 41);

        assert!(compare(&one, &one.clone(), LOCK, PROGRAM).is_ok());
    }

    #[test]
    fn another_pid_or_start_time_or_a_process_that_leads_no_group_is_not_foreground() {
        let one = sample(41, "t1", 41);

        assert_eq!(
            reason(compare(&one, &sample(42, "t1", 42), LOCK, PROGRAM)),
            "not_foreground"
        );
        assert_eq!(
            reason(compare(&one, &sample(41, "t2", 41), LOCK, PROGRAM)),
            "not_foreground"
        );
        let follower = sample(41, "t1", 7);
        assert_eq!(
            reason(compare(&follower, &follower.clone(), LOCK, PROGRAM)),
            "not_foreground"
        );
    }

    #[test]
    fn another_file_at_fd_3_or_another_executable_fails_with_its_reason() {
        let one = sample(41, "t1", 41);
        let mut closed = one.clone();
        closed.fd3 = None;
        let mut copied = one.clone();
        copied.txt = Some((1, 99));
        let mut unadoptable = one.clone();
        unadoptable.adoptable = false;

        assert_eq!(
            reason(compare(&one, &closed, LOCK, PROGRAM)),
            "lock_not_held"
        );
        assert_eq!(
            reason(compare(&unadoptable, &one, LOCK, PROGRAM)),
            "lock_not_held"
        );
        assert_eq!(reason(compare(&one, &copied, LOCK, PROGRAM)), "re_exec");
    }

    #[test]
    fn lsof_output_is_read_for_the_named_descriptor_only() {
        let text = "p41\nfcwd\nD0x1000010\ni2\nftxt\nD0x1000010\ni77\nf3\nD0x1000011\ni88\n";

        assert_eq!(parse_lsof(text, "txt"), Some((0x0100_0010, 77)));
        assert_eq!(parse_lsof(text, "3"), Some((0x0100_0011, 88)));
        assert_eq!(parse_lsof(text, "4"), None);
    }
}
