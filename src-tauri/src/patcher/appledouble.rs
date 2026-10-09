//! Find and remove macOS AppleDouble ("._name") sidecar files inside an install.
//!
//! exFAT -- the usual format for an external game drive -- cannot store
//! extended attributes, so macOS writes them to a hidden sidecar next to each
//! file: `basecontent.idx` gets `._basecontent.idx`. Modern macOS stamps every
//! file an app creates with a `com.apple.provenance` attribute, so when Steam
//! or Wargaming Game Center runs under CrossOver and updates the game, every
//! file it writes grows a sidecar.
//!
//! macOS hides these files from itself. Wine does not -- to the game they are
//! ordinary files. The client's pack-file loader takes every `*.idx` in
//! `bin/<build>/idx`, so `._basecontent.idx` is parsed as a pack index. It is
//! not one, the whole resource setup fails, and the client dies with:
//!
//!     No resource paths are loaded from command line or paths.xml
//!
//! A sidecar holds only metadata, never game data, so removing one is always
//! safe. A file only counts as a sidecar if its name starts with "._" AND it
//! begins with the AppleDouble magic number; nothing else is ever touched.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use walkdir::WalkDir;

pub const PREFIX: &str = "._";
pub const MAGIC: [u8; 4] = [0x00, 0x05, 0x16, 0x07];

/// The symptom, verbatim, so someone searching for the crash text lands here.
pub const SYMPTOM: &str = "No resource paths are loaded from command line or paths.xml";

/// Progress reported while walking a large install.
#[derive(Debug, Clone)]
pub struct ScanProgress {
    /// Files examined so far, so the UI can show movement even when nothing
    /// interesting has been found yet.
    pub scanned: u64,
    pub found: u64,
    pub current: PathBuf,
}

/// How many files to walk between progress reports.
const PROGRESS_EVERY: u64 = 2048;

/// Every AppleDouble sidecar under an install root.
#[derive(Debug, Clone)]
pub struct Scan {
    pub root: PathBuf,
    pub sidecars: Vec<PathBuf>,
    pub live_build: Option<String>,
    /// Set when the walk was cut short, so the UI can say the result is partial.
    pub cancelled: bool,
}

impl Scan {
    pub fn count(&self) -> usize {
        self.sidecars.len()
    }

    /// Sidecars inside bin/<live build>/, where the client reads them.
    pub fn in_live_build(&self) -> Vec<PathBuf> {
        let Some(live_build) = &self.live_build else {
            return Vec::new();
        };
        let build_dir = self.root.join("bin").join(live_build);
        self.sidecars
            .iter()
            .filter(|path| path.starts_with(&build_dir))
            .cloned()
            .collect()
    }
}

/// True only for a regular file named "._*" that starts with the magic.
pub fn is_sidecar(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !name.starts_with(PREFIX) {
        return false;
    }
    // `symlink_metadata` so a symlink to a real file is never treated as one.
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes.len() >= MAGIC.len() && bytes[..MAGIC.len()] == MAGIC
}

/// Walk the whole install. Directory entries only, so this is quick even over
/// the ~80 GB of res_packages.
pub fn scan(root: &Path, live_build: Option<&str>) -> Scan {
    scan_with_progress(root, live_build, &mut |_| {}, &AtomicBool::new(false))
}

/// `scan`, but reporting progress and honouring a cancellation flag.
///
/// The walk is over an entire game install, so the GUI has to be able to show
/// movement and let the user stop it.
pub fn scan_with_progress(
    root: &Path,
    live_build: Option<&str>,
    progress: &mut dyn FnMut(ScanProgress),
    cancel: &AtomicBool,
) -> Scan {
    let mut result = Scan {
        root: root.to_path_buf(),
        sidecars: Vec::new(),
        live_build: live_build.map(str::to_string),
        cancelled: false,
    };

    let mut scanned: u64 = 0;
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if cancel.load(Ordering::Relaxed) {
            result.cancelled = true;
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        scanned += 1;
        let path = entry.into_path();

        let is_candidate = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(PREFIX));

        if is_candidate && is_sidecar(&path) {
            result.sidecars.push(path);
            progress(ScanProgress {
                scanned,
                found: result.sidecars.len() as u64,
                current: result.sidecars.last().cloned().unwrap_or_default(),
            });
        } else if scanned % PROGRESS_EVERY == 0 {
            progress(ScanProgress {
                scanned,
                found: result.sidecars.len() as u64,
                current: path,
            });
        }
    }

    result.sidecars.sort();
    result
}

/// Remove the sidecar macOS created for a file this tool just wrote.
pub fn drop_sidecar(path: &Path) {
    let Some(name) = path.file_name() else {
        return;
    };
    let mut prefixed = OsString::from(PREFIX);
    prefixed.push(name);
    let sidecar = path.with_file_name(prefixed);
    if is_sidecar(&sidecar) {
        // Cosmetic for a DLL; `clean` will report it if it persists.
        let _ = std::fs::remove_file(&sidecar);
    }
}

/// Delete verified sidecars. Returns a message per file that failed.
pub fn remove(paths: &[PathBuf]) -> Vec<String> {
    let mut failures = Vec::new();
    for path in paths {
        // Vanished or changed since the scan; leave it alone.
        if !is_sidecar(path) {
            continue;
        }
        if let Err(exc) = std::fs::remove_file(path) {
            failures.push(format!("{}: {}", path.display(), exc));
        }
    }
    failures
}
