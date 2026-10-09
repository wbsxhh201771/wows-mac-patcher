//! Small filesystem helpers.
//!
//! These exist so the crate does not need the `tempfile` dependency: the only
//! thing we ever needed from it was "write this file atomically", which is a
//! temp file in the same directory plus a rename.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A path in `dir` that nothing else is using.
///
/// Uniqueness comes from the pid plus a process-wide counter. A leftover file
/// from a crashed run is never reused, because `atomic_write` opens with
/// `create_new`.
pub fn unique_temp_path(dir: &Path, prefix: &str, suffix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(
        "{}{}-{}{}",
        prefix,
        std::process::id(),
        sequence,
        suffix
    ))
}

/// Write `data` to `path`, atomically.
///
/// A seek-and-write in place would leave a half-written DLL if an external
/// drive hiccups mid-operation, so the bytes land in a sibling temp file first
/// and are renamed over the target.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let temp = unique_temp_path(parent, ".wowspatch-", ".tmp");

    let outcome = write_then_rename(path, &temp, data);
    if outcome.is_err() {
        let _ = fs::remove_file(&temp);
    }
    outcome
}

fn write_then_rename(path: &Path, temp: &Path, data: &[u8]) -> io::Result<()> {
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temp)?;
        file.write_all(data)?;
        file.flush()?;
        file.sync_all()?;
    }

    // exFAT carries no permission bits; not being able to copy them is fine.
    if let Ok(metadata) = fs::metadata(path) {
        let _ = fs::set_permissions(temp, metadata.permissions());
    }

    fs::rename(temp, path)
}
