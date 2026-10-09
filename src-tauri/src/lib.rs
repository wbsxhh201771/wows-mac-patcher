//! Tauri command surface.
//!
//! Everything here is a thin wrapper around `patcher`, which owns the actual
//! behaviour and is tested without a GUI. Two rules are enforced at this layer:
//!
//!   * every command that touches the filesystem runs on a blocking thread, so
//!     hashing a DLL or walking an install never freezes the window;
//!   * the manifest is re-read on every command rather than cached, so external
//!     changes to `state.json` are picked up immediately.

pub mod patcher;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use patcher::diag::{self, EnvReport, LogReport};
use patcher::dto::{InstallDto, ReportDto};
use patcher::manifest::Manifest;
use patcher::{locate, patch};

#[derive(Default)]
struct AppState {
    /// Lets the UI stop an in-flight sidecar walk.
    ///
    /// An `Arc` rather than a bare `AtomicBool` so a clone can be moved into
    /// the blocking closure, which must outlive the borrowed `State`.
    scan_cancel: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
struct ScanProgressDto {
    scanned: u64,
    found: u64,
    current: String,
}

#[derive(Serialize, serde::Deserialize)]
struct PatchOutcome {
    install: String,
    build: Option<String>,
    version: Option<String>,
    dll: String,
    backup: String,
    export: String,
    export_rva: u32,
    file_offset: u64,
    original_bytes: String,
    patched_bytes: String,
    sha256_original: String,
    sha256_patched: String,
    size: u64,
    patched_at: String,
}

#[derive(Serialize, serde::Deserialize)]
struct RestoreOutcome {
    dll: String,
    backup: String,
    sha256_restored: String,
    restored_at: String,
}

#[derive(Serialize, serde::Deserialize)]
struct CleanOutcome {
    found: usize,
    removed: usize,
    failures: Vec<String>,
}

/// Run blocking filesystem work off the UI thread.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|exc| format!("后台任务失败：{}", exc))?
}

fn deserialise<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|exc| format!("结果解析失败：{}", exc))
}

// Commands -----------------------------------------------------------------

#[tauri::command]
async fn list_installs() -> Result<Vec<InstallDto>, String> {
    blocking(|| {
        Ok(locate::discover()
            .iter()
            .map(InstallDto::from)
            .collect::<Vec<_>>())
    })
    .await
}

#[tauri::command]
async fn resolve_install(path: String) -> Result<InstallDto, String> {
    blocking(move || {
        locate::from_path(Path::new(&path))
            .map(|install| InstallDto::from(&install))
            .map_err(|exc| exc.to_string())
    })
    .await
}

/// Read-only. This is the dry run, and it never writes anything.
#[tauri::command]
async fn status(
    app: AppHandle,
    state: State<'_, AppState>,
    install_path: Option<String>,
) -> Result<ReportDto, String> {
    let cancel = state.scan_cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    blocking(move || {
        let manifest = Manifest::new();
        let install = patch::resolve_install(install_path.as_deref(), Some(&manifest))
            .map_err(|exc| exc.0)?;

        // `appledouble` already throttles to one callback per 2048 files (and
        // one per find), so every callback here is worth forwarding.
        let report = patch::resolve_with(
            install,
            Some(&manifest),
            &mut |progress| {
                let _ = app.emit(
                    "scan://progress",
                    ScanProgressDto {
                        scanned: progress.scanned,
                        found: progress.found,
                        current: progress.current.display().to_string(),
                    },
                );
            },
            &cancel,
        );
        Ok(ReportDto::from(&report))
    })
    .await
}

/// Back up and patch. The caller must pass the SHA-256 it displayed.
///
/// Re-checking it here is what stops a confirmation dialog left open across a
/// game update from writing over the new binary.
#[tauri::command]
async fn patch(
    install_path: Option<String>,
    expected_sha256: String,
) -> Result<PatchOutcome, String> {
    blocking(move || {
        let mut manifest = Manifest::new();
        let install = patch::resolve_install(install_path.as_deref(), Some(&manifest))
            .map_err(|exc| exc.0)?;
        let report = patch::resolve(install, Some(&manifest));

        let actual = report
            .target
            .as_ref()
            .map(|target| target.sha256.clone())
            .unwrap_or_default();
        if !expected_sha256.is_empty() && actual != expected_sha256 {
            return Err(
                "DLL 在界面读取之后发生了变化（游戏可能刚更新过）。请刷新状态后重试。".to_string(),
            );
        }

        let record = patch::apply(&report, &mut manifest).map_err(|exc| exc.0)?;
        deserialise(record)
    })
    .await
}

#[tauri::command]
async fn restore(install_path: Option<String>) -> Result<RestoreOutcome, String> {
    blocking(move || {
        let mut manifest = Manifest::new();
        let install = patch::resolve_install(install_path.as_deref(), Some(&manifest))
            .map_err(|exc| exc.0)?;
        let report = patch::resolve(install, Some(&manifest));
        if report.state == patch::ORIGINAL {
            return Err("当前未打补丁，无需还原。".to_string());
        }
        if !report.can_restore() {
            return Err(format!(
                "当前状态（{}）无法还原。",
                patch::state_help(&report.state)
            ));
        }
        let result = patch::restore(&report, &mut manifest).map_err(|exc| exc.0)?;
        deserialise(result)
    })
    .await
}

#[tauri::command]
async fn clean(install_path: Option<String>) -> Result<CleanOutcome, String> {
    blocking(move || {
        let manifest = Manifest::new();
        let install = patch::resolve_install(install_path.as_deref(), Some(&manifest))
            .map_err(|exc| exc.0)?;
        let mut report = patch::resolve(install, Some(&manifest));
        let result = patch::clean(&mut report).map_err(|exc| exc.0)?;
        deserialise(result)
    })
    .await
}

/// Environment check. `include_wine` starts a wineserver, so it is opt-in.
#[tauri::command]
async fn check_env(include_wine: bool, bottle: String) -> Result<EnvReport, String> {
    blocking(move || Ok(diag::env_report(include_wine, &bottle))).await
}

#[tauri::command]
async fn scan_log(install_path: Option<String>) -> Result<LogReport, String> {
    blocking(move || {
        let manifest = Manifest::new();
        let root = match patch::resolve_install(install_path.as_deref(), Some(&manifest)) {
            Ok(install) => install.root,
            Err(exc) => return Err(exc.0),
        };
        Ok(diag::log_report(&root))
    })
    .await
}

#[tauri::command]
fn running_processes() -> Vec<String> {
    diag::running_processes()
}

#[tauri::command]
fn state_path() -> String {
    patcher::manifest::state_file().display().to_string()
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) {
    state.scan_cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .args(["-R", &path])
        .spawn()
        .map(|_| ())
        .map_err(|exc| format!("无法打开 Finder：{}", exc))
}

/// Open a Terminal window with the command pre-typed.
///
/// The hostname fix needs root, and a GUI must never escalate silently. The
/// user sees the exact command, and this only saves them typing it.
#[tauri::command]
fn open_terminal(command: String) -> Result<(), String> {
    let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("tell application \"Terminal\" to do script \"{}\"", escaped);
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .spawn()
        .map(|_| ())
        .map_err(|exc| format!("无法打开终端：{}", exc))
}

/// Native folder picker, via `choose folder`.
///
/// Returns `None` when the user cancels, which is not an error. Using
/// osascript instead of `tauri-plugin-dialog` keeps a whole plugin out of the
/// build for the one thing we need from it.
#[tauri::command]
async fn pick_folder(prompt: String) -> Result<Option<String>, String> {
    blocking(move || {
        let script = format!("POSIX path of (choose folder with prompt \"{}\")", prompt);
        let output = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|exc| format!("无法打开选择器：{}", exc))?;

        // Cancelling makes osascript exit non-zero; that is not a failure.
        if !output.status.success() {
            return Ok(None);
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if path.is_empty() { None } else { Some(path) })
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            // Bring the main window forward on launch.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_installs,
            resolve_install,
            status,
            patch,
            restore,
            clean,
            check_env,
            scan_log,
            running_processes,
            state_path,
            cancel_scan,
            reveal_in_finder,
            open_terminal,
            pick_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
