//! `binary`: the artifact is the executable itself, written to the first
//! path in `executables`, which has one component (spec 0018, *Format*).

use std::io::{Seek, SeekFrom};

use stackly_platform::ReadSeekFile;

use super::ArchiveError;
use super::engine::{ArchiveMode, Extraction, Member};
use super::source::Tracked;

/// Reads a `binary` artifact into `run`.
pub(super) fn read(
    input: &mut dyn ReadSeekFile,
    run: &mut Extraction<'_>,
) -> Result<(), ArchiveError> {
    let spec = run.spec();
    let name = match spec.executables.first() {
        Some(executable) if executable.depth() == 1 && spec.archive_root.is_none() => {
            executable.as_str().to_owned()
        }
        _ => {
            return Err(ArchiveError::InvalidSpec {
                reason: "a binary artifact needs a first executable of one component and no archiveRoot",
            });
        }
    };
    let mut file = Tracked::new(input, run.source_error());
    file.seek(SeekFrom::Start(0))
        .map_err(|_| run.read_error(name.as_bytes()))?;
    let size = file.len();
    run.add(
        name.as_bytes(),
        ArchiveMode::Unknown,
        Member::File {
            size,
            data: &mut file,
        },
    )
}
