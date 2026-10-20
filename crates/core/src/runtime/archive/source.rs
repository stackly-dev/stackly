//! The artifact file as the format readers see it. Every I/O error of the
//! file itself is noted, so an error a decoder passes on can be told apart
//! from a broken archive.

use std::cell::RefCell;
use std::io::{self, Read, Seek, SeekFrom};
use std::rc::Rc;

use stackly_platform::ReadSeekFile;

/// The first I/O error the artifact file returned, shared between the
/// reader and the extraction.
pub(super) type SourceError = Rc<RefCell<Option<(io::ErrorKind, String)>>>;

/// The artifact file, noting its own I/O errors in a [`SourceError`].
pub(super) struct Tracked<'a> {
    file: &'a mut dyn ReadSeekFile,
    error: SourceError,
}

impl<'a> Tracked<'a> {
    pub(super) fn new(file: &'a mut dyn ReadSeekFile, error: SourceError) -> Self {
        Self { file, error }
    }

    /// The size of the file when it was opened.
    pub(super) fn len(&self) -> u64 {
        self.file.len()
    }

    fn note<T>(&self, result: io::Result<T>) -> io::Result<T> {
        if let Err(err) = &result {
            let mut slot = self.error.borrow_mut();
            if slot.is_none() && err.kind() != io::ErrorKind::Interrupted {
                *slot = Some((err.kind(), err.to_string()));
            }
        }
        result
    }
}

impl Read for Tracked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let result = self.file.read(buf);
        self.note(result)
    }
}

impl Seek for Tracked<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let result = self.file.seek(pos);
        self.note(result)
    }
}
