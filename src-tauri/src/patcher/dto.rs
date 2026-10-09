//! Serialisable views of the core types.
//!
//! `patch::Report` is built for decisions, not for shipping over IPC: it holds
//! `PathBuf`s, raw bytes, and a nested `Install`. These DTOs are the shape the
//! frontend actually wants.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use super::appledouble::Scan;
use super::locate::Install;
use super::patch::{self, DllInfo, Report, Site};

#[derive(Debug, Clone, Serialize)]
pub struct InstallDto {
    pub root: String,
    pub build: Option<String>,
    pub version: Option<String>,
    pub build_source: String,
    pub available_builds: Vec<String>,
    pub patchable_builds: Vec<String>,
    pub incomplete_builds: Vec<String>,
}

impl From<&Install> for InstallDto {
    fn from(install: &Install) -> Self {
        InstallDto {
            root: install.root.display().to_string(),
            build: install.build.clone(),
            version: install.version.clone(),
            build_source: install.build_source.clone(),
            available_builds: install.available_builds.clone(),
            patchable_builds: install.patchable_builds.clone(),
            incomplete_builds: install.incomplete_builds(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteDto {
    pub export_name: String,
    pub export_ordinal: u32,
    pub export_rva: u32,
    pub file_offset: u64,
    pub section: String,
    pub section_is_code: bool,
    /// Pre-rendered `31 C0 C3`, so the UI never formats bytes itself.
    pub current_bytes: String,
    pub wine_import_dll: Option<String>,
    pub wine_iat_rva: Option<u32>,
    pub wine_string_rva: Option<u32>,
    pub wine_reference: Option<String>,
    pub is_patched: bool,
}

impl From<&Site> for SiteDto {
    fn from(site: &Site) -> Self {
        SiteDto {
            export_name: site.export_name.clone(),
            export_ordinal: site.export_ordinal,
            export_rva: site.export_rva,
            file_offset: site.file_offset as u64,
            section: site.section.clone(),
            section_is_code: site.section_is_code,
            current_bytes: patch::hexbytes(&site.current_bytes),
            wine_import_dll: site.wine_import_dll.clone(),
            wine_iat_rva: site.wine_iat_rva,
            wine_string_rva: site.wine_string_rva,
            wine_reference: site.wine_reference.clone(),
            is_patched: site.is_patched(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DllInfoDto {
    pub path: String,
    pub name: String,
    pub exists: bool,
    pub size: u64,
    pub sha256: String,
    pub error: Option<String>,
    pub site: Option<SiteDto>,
}

impl From<&DllInfo> for DllInfoDto {
    fn from(info: &DllInfo) -> Self {
        DllInfoDto {
            path: info.path.display().to_string(),
            name: info
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            exists: info.exists,
            size: info.size,
            sha256: info.sha256.clone(),
            error: info.error.clone(),
            site: info.site.as_ref().map(SiteDto::from),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarsDto {
    pub count: usize,
    pub in_live_build: usize,
    /// `(folder label, count)`, largest first.
    pub by_folder: Vec<(String, usize)>,
    /// True when the walk was stopped early, so the count is partial.
    pub cancelled: bool,
}

/// Sidecar counts per top-level folder (per build under bin/), largest first.
fn sidecar_groups(scan: &Scan) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for path in &scan.sidecars {
        let Ok(relative) = path.strip_prefix(&scan.root) else {
            continue;
        };
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();

        let label = if parts.len() == 1 {
            "(install root)".to_string()
        } else if parts[0] == "bin" && parts.len() > 2 {
            format!("bin/{}/", parts[1])
        } else {
            format!("{}/", parts[0])
        };
        *counts.entry(label).or_insert(0) += 1;
    }

    let mut groups: Vec<(String, usize)> = counts.into_iter().collect();
    groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    groups
}

impl From<&Scan> for SidecarsDto {
    fn from(scan: &Scan) -> Self {
        SidecarsDto {
            count: scan.count(),
            in_live_build: scan.in_live_build().len(),
            by_folder: sidecar_groups(scan),
            cancelled: scan.cancelled,
        }
    }
}

/// Everything the UI needs to render a status screen.
#[derive(Debug, Clone, Serialize)]
pub struct ReportDto {
    pub state: String,
    pub state_help: String,
    pub error: Option<String>,
    pub notes: Vec<String>,
    pub can_patch: bool,
    pub can_restore: bool,
    pub needs_clean: bool,
    pub install: Option<InstallDto>,
    pub target: Option<DllInfoDto>,
    pub backup: Option<DllInfoDto>,
    pub record: Option<Value>,
    pub sidecars: Option<SidecarsDto>,
    /// The three bytes this tool writes, and what they mean. Shipped to the
    /// frontend so the confirmation dialog can show them without hardcoding.
    pub patch_bytes: String,
    pub patch_disasm: String,
    pub export_needle: String,
    pub wine_import: String,
}

impl From<&Report> for ReportDto {
    fn from(report: &Report) -> Self {
        ReportDto {
            state: report.state.clone(),
            state_help: patch::state_help(&report.state).to_string(),
            error: report.error.clone(),
            notes: report.notes.clone(),
            can_patch: report.can_patch(),
            can_restore: report.can_restore(),
            needs_clean: report.needs_clean(),
            install: report.install.as_ref().map(InstallDto::from),
            target: report.target.as_ref().map(DllInfoDto::from),
            backup: report.backup.as_ref().map(DllInfoDto::from),
            record: report.record.clone(),
            sidecars: report.sidecars.as_ref().map(SidecarsDto::from),
            patch_bytes: patch::hexbytes(&patch::PATCH_BYTES),
            patch_disasm: patch::PATCH_DISASM.to_string(),
            export_needle: patch::EXPORT_NEEDLE.to_string(),
            wine_import: patch::WINE_IMPORT.to_string(),
        }
    }
}
