//! Durable record of what was patched, where, and what it looked like before.
//!
//! The manifest is what makes `restore` honest. A backup file alone cannot
//! tell you whether it belongs to the build currently installed -- the hashes
//! recorded here can.
//!
//! The document is carried as a raw `serde_json::Value` and only the fields we
//! understand are touched. Unknown keys written by a future version round-trip
//! untouched; field names are never renamed or added as required.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub const APP_NAME: &str = "WoWsMacPatcher";
pub const SCHEMA_VERSION: u64 = 1;

/// How many records to keep before dropping the oldest.
const MAX_RECORDS: usize = 50;

pub fn state_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join("Library")
            .join("Application Support")
            .join(APP_NAME);
    }
    if let Some(base) = std::env::var_os("XDG_STATE_HOME")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(base).join(APP_NAME);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".local")
        .join("state")
        .join(APP_NAME)
}

pub fn state_file() -> PathBuf {
    state_dir().join("state.json")
}

/// Local timestamp like `2026-10-06T19:55:12+0800`.
pub fn now() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%z")
        .to_string()
}

/// A tiny append-mostly JSON store keyed by DLL path.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub path: PathBuf,
    pub data: Value,
}

impl Default for Manifest {
    fn default() -> Self {
        Self::new()
    }
}

impl Manifest {
    pub fn new() -> Self {
        Self::with_path(state_file())
    }

    pub fn with_path(path: PathBuf) -> Self {
        let mut manifest = Manifest {
            path,
            data: json!({
                "schema": SCHEMA_VERSION,
                "records": [],
                "last_install": Value::Null,
            }),
        };
        manifest.load();
        manifest
    }

    fn load(&mut self) {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return;
        };
        // A corrupt manifest must never block a restore; the backup file and
        // its on-disk bytes remain the source of truth.
        let Ok(loaded) = serde_json::from_str::<Value>(&text) else {
            return;
        };
        let Some(object) = loaded.as_object() else {
            return;
        };
        if !object.get("records").is_some_and(Value::is_array) {
            return;
        }

        self.data = loaded;
        let object = self.data.as_object_mut().expect("checked above");
        object
            .entry("schema")
            .or_insert_with(|| json!(SCHEMA_VERSION));
        object.entry("last_install").or_insert(Value::Null);
    }

    pub fn save(&self) -> std::io::Result<()> {
        let parent = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;

        let payload = serde_json::to_string_pretty(&self.data).map_err(std::io::Error::other)?;
        let mut bytes = payload.into_bytes();
        bytes.push(b'\n');

        super::fsutil::atomic_write(&self.path, &bytes)
    }

    // ------------------------------------------------------------------ //

    pub fn records(&self) -> &Vec<Value> {
        self.data
            .get("records")
            .and_then(Value::as_array)
            .expect("records is always an array")
    }

    pub fn last_install(&self) -> Option<String> {
        self.data
            .get("last_install")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    pub fn remember_install(&mut self, root: &Path) {
        if let Some(object) = self.data.as_object_mut() {
            object.insert(
                "last_install".to_string(),
                Value::String(root.display().to_string()),
            );
        }
    }

    fn index_for(&self, dll_path: &Path) -> Option<usize> {
        let key = dll_path.display().to_string();
        self.records()
            .iter()
            .rposition(|record| record.get("dll").and_then(Value::as_str) == Some(key.as_str()))
    }

    /// Most recent record for this DLL path, if any.
    pub fn record_for(&self, dll_path: &Path) -> Option<Value> {
        self.index_for(dll_path).map(|index| self.records()[index].clone())
    }

    pub fn put(&mut self, mut record: Value) {
        if let Some(object) = record.as_object_mut() {
            object
                .entry("recorded_at")
                .or_insert_with(|| Value::String(now()));
        }
        let Some(records) = self.data.get_mut("records").and_then(Value::as_array_mut) else {
            return;
        };
        records.push(record);
        // Keep the file from growing without bound across many game updates.
        if records.len() > MAX_RECORDS {
            let excess = records.len() - MAX_RECORDS;
            records.drain(..excess);
        }
    }

    pub fn mark_restored(&mut self, dll_path: &Path) {
        let Some(index) = self.index_for(dll_path) else {
            return;
        };
        let stamp = Value::String(now());
        if let Some(object) = self
            .data
            .get_mut("records")
            .and_then(Value::as_array_mut)
            .and_then(|records| records.get_mut(index))
            .and_then(Value::as_object_mut)
        {
            object.insert("restored_at".to_string(), stamp);
        }
    }
}
