//! Find World of Warships installs, and work out which build is actually live.
//!
//! Two things matter here:
//!
//!   1. Users install to odd places -- CrossOver bottles, external drives,
//!      folders with spaces in the name. Auto-detection saves them from
//!      pasting paths.
//!
//!   2. `bin/` accumulates old build directories after updates. Patching a
//!      stale one silently does nothing and looks like "the patcher is
//!      broken", so the live build is resolved deliberately and the source of
//!      that answer is reported to the user.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

pub const DLL_NAME: &str = "platform64.dll";
pub const BACKUP_NAME: &str = "platform64_bck.dll";

/// Folder names vary by installer and by user: "World_of_Warships" (WGC),
/// "World of Warships" (Steam), and assorted hand-made variants. Compare on a
/// normalised prefix instead of an exact list, which also tolerates suffixes
/// like "World of Warships PT" for the public test client. "World of
/// Warplanes" does not normalise to this prefix, so it will not be picked up
/// by mistake.
pub const INSTALL_PREFIX: &str = "worldofwarship";

pub const BUILD_READY: &str = "ready to patch";
pub const BUILD_PATCHED: &str = "already patched";

#[derive(Debug, Clone)]
pub enum LocateError {
    /// The user-supplied path does not exist.
    NotFound(String),
    /// The path exists but is not a World of Warships install.
    NotAnInstall(String),
}

impl fmt::Display for LocateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocateError::NotFound(message) | LocateError::NotAnInstall(message) => {
                f.write_str(message)
            }
        }
    }
}

impl std::error::Error for LocateError {}

fn version_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\b\d+\.\d+\.\d+\.\d+\b").expect("valid version regex"))
}

/// A World of Warships installation directory.
#[derive(Debug, Clone)]
pub struct Install {
    pub root: PathBuf,
    pub build: Option<String>,
    pub version: Option<String>,
    pub build_source: String,
    pub available_builds: Vec<String>,
    pub patchable_builds: Vec<String>,
}

impl Install {
    /// Build directories with no bin64/platform64.dll in them.
    ///
    /// Updates leave these behind. Selecting one produces a baffling "file not
    /// found", so they are excluded from the choice and reported.
    pub fn incomplete_builds(&self) -> Vec<String> {
        self.available_builds
            .iter()
            .filter(|build| !self.patchable_builds.contains(build))
            .cloned()
            .collect()
    }

    pub fn dll(&self) -> Option<PathBuf> {
        let build = self.build.as_ref()?;
        Some(self.root.join("bin").join(build).join("bin64").join(DLL_NAME))
    }

    pub fn backup(&self) -> Option<PathBuf> {
        let dll = self.dll()?;
        Some(dll.parent()?.join(BACKUP_NAME))
    }

    pub fn has_stale_builds(&self) -> bool {
        self.available_builds.len() > 1
    }
}

/// Cheap structural check that `path` is a WoWs install root.
pub fn looks_like_install(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    if path.join("game_info.xml").is_file() {
        return true;
    }
    let binaries = path.join("bin");
    if binaries.is_dir() {
        return safe_iterdir(&binaries)
            .iter()
            .any(|child| is_numeric_name(child));
    }
    false
}

/// `read_dir()` that shrugs off unreadable or unmounted directories.
pub fn safe_iterdir(path: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(path) {
        Ok(entries) => entries.filter_map(|entry| entry.ok()).map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    }
}

fn is_numeric_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()))
}

/// Build directory names are pure ASCII digits (not Unicode numeric chars).
fn name_of(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn normalise(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

pub(crate) fn children_named_like_install(parent: &Path) -> Vec<PathBuf> {
    safe_iterdir(parent)
        .into_iter()
        .filter(|child| child.is_dir() && normalise(&name_of(child)).starts_with(INSTALL_PREFIX))
        .collect()
}

/// Directories worth scanning for an install, in order of likelihood.
pub fn candidate_roots() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    let mut parents: Vec<PathBuf> = Vec::new();
    let add_game_parents = |parents: &mut Vec<PathBuf>, base: &Path| {
        parents.push(base.to_path_buf());
        parents.push(base.join("Games"));
        // Steam keeps games under <library>/steamapps/common. Users relocate
        // libraries constantly, so check the usual spellings under each base.
        for library in ["SteamLibrary", "Steam", "steamapps"] {
            parents.push(base.join(library).join("steamapps").join("common"));
        }
        parents.push(base.join("steamapps").join("common"));
    };

    // External drives -- the common case for a game this size.
    for volume in safe_iterdir(Path::new("/Volumes")) {
        if volume.is_dir() {
            add_game_parents(&mut parents, &volume);
        }
    }

    // Steam's default macOS location.
    add_game_parents(
        &mut parents,
        &home
            .join("Library")
            .join("Application Support")
            .join("Steam"),
    );

    // CrossOver bottles.
    let bottles = home
        .join("Library")
        .join("Application Support")
        .join("CrossOver")
        .join("Bottles");
    for bottle in safe_iterdir(&bottles) {
        let drive_c = bottle.join("drive_c");
        if !drive_c.is_dir() {
            continue;
        }
        add_game_parents(&mut parents, &drive_c);
        for program_files in ["Program Files", "Program Files (x86)"] {
            add_game_parents(&mut parents, &drive_c.join(program_files));
        }
    }

    add_game_parents(&mut parents, &home);

    let mut seen = std::collections::HashSet::new();
    parents
        .into_iter()
        .filter(|parent| seen.insert(parent.clone()))
        .collect()
}

/// Every install we can find, de-duplicated and build-resolved.
pub fn discover() -> Vec<Install> {
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for parent in candidate_roots() {
        for candidate in children_named_like_install(&parent) {
            let resolved = candidate
                .canonicalize()
                .unwrap_or_else(|_| candidate.clone());
            if !seen.insert(resolved) || !looks_like_install(&candidate) {
                continue;
            }
            found.push(inspect(&candidate));
        }
    }
    found
}

/// Numeric build directories under bin/, newest first.
pub fn list_builds(root: &Path) -> Vec<String> {
    let mut builds: Vec<String> = safe_iterdir(&root.join("bin"))
        .into_iter()
        .filter(|child| is_numeric_name(child))
        .map(|child| name_of(&child))
        .collect();
    builds.sort_by(|a, b| {
        let a = a.parse::<u64>().unwrap_or(0);
        let b = b.parse::<u64>().unwrap_or(0);
        b.cmp(&a)
    });
    builds
}

/// Of `builds`, those that actually contain the DLL we patch.
pub fn list_patchable_builds(root: &Path, builds: &[String]) -> Vec<String> {
    builds
        .iter()
        .filter(|build| root.join("bin").join(build).join("bin64").join(DLL_NAME).is_file())
        .cloned()
        .collect()
}

/// Resolve the live build for an install root.
pub fn inspect(root: &Path) -> Install {
    let available_builds = list_builds(root);
    let patchable_builds = list_patchable_builds(root, &available_builds);
    let pool: Vec<String> = if patchable_builds.is_empty() {
        available_builds.clone()
    } else {
        patchable_builds.clone()
    };

    let mut install = Install {
        root: root.to_path_buf(),
        build: None,
        version: None,
        build_source: "unknown".to_string(),
        available_builds,
        patchable_builds,
    };

    let text = std::fs::read_to_string(root.join("game_info.xml")).unwrap_or_default();

    if !text.is_empty() {
        if let Some(found) = version_pattern().find(&text) {
            install.version = Some(found.as_str().to_string());
        }

        // Rather than guess at game_info.xml's schema (which Wargaming has
        // changed before), cross-reference: the live build number appears in
        // the file *and* exists as a bin/ directory. That intersection is a
        // far more stable signal than any particular element name.
        for build in &pool {
            if Regex::new(&format!(r"\b{}\b", regex::escape(build)))
                .map(|pattern| pattern.is_match(&text))
                .unwrap_or(false)
            {
                install.build = Some(build.clone());
                install.build_source = "game_info.xml".to_string();
                break;
            }
        }

        if install.build.is_none() {
            if let Some(version) = &install.version {
                let tail = version.rsplit('.').next().unwrap_or_default().to_string();
                if pool.contains(&tail) {
                    install.build = Some(tail);
                    install.build_source = "game_info.xml (version tail)".to_string();
                }
            }
        }
    }

    if install.build.is_none() {
        if let Some(first) = pool.first() {
            install.build = Some(first.clone());
            // Steam installs have no game_info.xml at all, so this is the
            // normal path there rather than an unusual fallback.
            install.build_source = "highest-numbered bin/ directory with the DLL".to_string();
            if install.version.is_none() {
                install.version = Some(first.clone());
            }
        }
    }

    install
}

/// Build an Install from a user-supplied path.
///
/// Accepts the install root, or any path inside it (people paste the bin64
/// folder, or the DLL itself, surprisingly often).
pub fn from_path(path: &Path) -> Result<Install, LocateError> {
    let path = expand_user(path);
    if !path.exists() {
        return Err(LocateError::NotFound(format!(
            "no such path: {}",
            path.display()
        )));
    }

    let start = if path.is_file() {
        path.parent().map(Path::to_path_buf).unwrap_or(path.clone())
    } else {
        path.clone()
    };

    for candidate in std::iter::once(start.clone()).chain(start.ancestors().skip(1).map(Path::to_path_buf)) {
        if looks_like_install(&candidate) {
            return Ok(inspect(&candidate));
        }
    }

    Err(LocateError::NotAnInstall(format!(
        "{} does not look like a World of Warships install \
         (no game_info.xml and no numbered bin/ directories)",
        path.display()
    )))
}

/// Expand a leading `~` to the home directory.
fn expand_user(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}
