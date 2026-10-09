//! The patch engine: resolve, classify, apply, restore.
//!
//! The only byte-level change this tool ever makes is three bytes at the entry
//! of one exported function:
//!
//! ```text
//!     31 C0        xor eax, eax
//!     C3           ret
//! ```
//!
//! Making `getHostVersionFromWine()` return 0 stops `WorldOfWarships64.exe`
//! from concluding that the Wine host is Darwin, which is what triggers the
//! "macOS support has been discontinued" message.
//!
//! The offset of that function is resolved from the DLL's export table on
//! every single run. Offsets are never remembered between builds -- a
//! hardcoded offset would corrupt the DLL the first time Wargaming ships an
//! update.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::appledouble::{self, Scan};
use super::locate::{self, Install};
use super::manifest::Manifest;
use super::pe;

pub const PATCH_BYTES: [u8; 3] = [0x31, 0xC0, 0xC3]; // xor eax, eax ; ret
pub const PATCH_DISASM: &str = "xor eax, eax ; ret";
pub const EXPORT_NEEDLE: &str = "getHostVersionFromWine";
pub const WINE_IMPORT: &str = "wine_get_host_version";

/// The game ships through both Steam and Wargaming Game Center, and the two
/// are NOT the same binary -- offsets differ between them for the same build
/// number. Never name only one launcher in user-facing text.
pub const REPAIR_HINT: &str = "Steam 的「验证游戏文件完整性」或 Wargaming Game Center 的「检查并修复」";

// States -------------------------------------------------------------------

pub const ORIGINAL: &str = "ORIGINAL";
pub const PATCHED: &str = "PATCHED";
pub const FOREIGN: &str = "FOREIGN";
pub const UNRESOLVED: &str = "UNRESOLVED";

pub fn state_help(state: &str) -> &'static str {
    match state {
        ORIGINAL => "尚未打补丁，可以打补丁",
        PATCHED => "已由本工具打补丁，可还原",
        FOREIGN => "已被修改，但不是本工具能安全处理的状态",
        UNRESOLVED => "找不到要打补丁的函数",
        _ => "",
    }
}

/// A refusal or failure that should be reported to the user verbatim.
#[derive(Debug, Clone)]
pub struct PatchError(pub String);

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PatchError {}

impl From<std::io::Error> for PatchError {
    fn from(exc: std::io::Error) -> Self {
        PatchError(exc.to_string())
    }
}

// Inspection ---------------------------------------------------------------

/// The resolved patch site inside a DLL.
#[derive(Debug, Clone)]
pub struct Site {
    pub export_name: String,
    pub export_ordinal: u32,
    pub export_rva: u32,
    pub file_offset: usize,
    pub section: String,
    pub section_is_code: bool,
    pub current_bytes: Vec<u8>,
    pub wine_import_dll: Option<String>,
    pub wine_iat_rva: Option<u32>,
    pub wine_string_rva: Option<u32>,
    pub wine_reference: Option<String>,
}

impl Site {
    pub fn is_patched(&self) -> bool {
        self.current_bytes == PATCH_BYTES
    }

    pub fn references_wine(&self) -> bool {
        self.wine_reference.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct DllInfo {
    pub path: PathBuf,
    pub exists: bool,
    pub size: u64,
    pub sha256: String,
    pub site: Option<Site>,
    pub error: Option<String>,
}

impl DllInfo {
    pub fn resolved(&self) -> bool {
        self.site.is_some()
    }
}

/// Everything `status` reports and everything `patch` decides on.
#[derive(Debug, Clone)]
pub struct Report {
    pub install: Option<Install>,
    pub target: Option<DllInfo>,
    pub backup: Option<DllInfo>,
    pub record: Option<Value>,
    pub sidecars: Option<Scan>,
    pub state: String,
    pub notes: Vec<String>,
    pub error: Option<String>,
}

impl Report {
    pub fn can_patch(&self) -> bool {
        self.state == ORIGINAL
    }

    pub fn can_restore(&self) -> bool {
        self.state == PATCHED && self.backup.as_ref().is_some_and(|backup| backup.exists)
    }

    pub fn needs_clean(&self) -> bool {
        self.sidecars.as_ref().is_some_and(|scan| scan.count() > 0)
    }
}

pub fn sha256_of(path: &Path) -> Result<String, PatchError> {
    let mut handle = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = handle.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn hexbytes(raw: &[u8]) -> String {
    raw.iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read a DLL and resolve the patch site, without touching it.
pub fn inspect_dll(path: &Path) -> DllInfo {
    let mut info = DllInfo {
        path: path.to_path_buf(),
        exists: false,
        size: 0,
        sha256: String::new(),
        site: None,
        error: None,
    };

    if !path.is_file() {
        return info;
    }
    info.exists = true;

    match path.metadata() {
        Ok(metadata) => info.size = metadata.len(),
        Err(exc) => {
            info.error = Some(exc.to_string());
            return info;
        }
    }
    match sha256_of(path) {
        Ok(digest) => info.sha256 = digest,
        Err(exc) => {
            info.error = Some(exc.to_string());
            return info;
        }
    }

    let image = match pe::PEFile::from_path(path) {
        Ok(image) => image,
        Err(exc) => {
            info.error = Some(exc.to_string());
            return info;
        }
    };

    if !image.is_64bit {
        info.error = Some(format!(
            "not a 64-bit PE image (found {})",
            image.machine_name()
        ));
        return info;
    }

    let matches = match image.find_exports(EXPORT_NEEDLE) {
        Ok(matches) => matches,
        Err(exc) => {
            info.error = Some(format!("could not read the export table: {}", exc));
            return info;
        }
    };

    if matches.is_empty() {
        info.error = Some(format!(
            "no export whose name contains '{}'. The detection may have been \
             renamed or moved in this build; stopping rather than guessing.",
            EXPORT_NEEDLE
        ));
        return info;
    }
    if matches.len() > 1 {
        info.error = Some(format!(
            "ambiguous: {} exports contain '{}' ({}). Refusing to guess which \
             one to patch.",
            matches.len(),
            EXPORT_NEEDLE,
            matches
                .iter()
                .map(|export| export.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        return info;
    }

    let export = matches.into_iter().next().expect("length checked above");
    if let Some(forwarder) = &export.forwarder {
        info.error = Some(format!(
            "'{}' is a forwarder to '{}', not code in this DLL",
            export.name, forwarder
        ));
        return info;
    }

    let offset = match image.rva_to_offset(export.rva) {
        Ok(offset) => offset,
        Err(exc) => {
            info.error = Some(exc.to_string());
            return info;
        }
    };

    let section = image.section_for_rva(export.rva);
    let end = (offset + PATCH_BYTES.len()).min(image.data.len());
    let current_bytes = image
        .data
        .get(offset..end)
        .unwrap_or(&[])
        .to_vec();

    let mut site = Site {
        export_name: export.name.clone(),
        export_ordinal: export.ordinal,
        export_rva: export.rva,
        file_offset: offset,
        section: section.map(|s| s.name.clone()).unwrap_or_else(|| "?".to_string()),
        section_is_code: section.is_some_and(|s| s.is_executable_code()),
        current_bytes,
        wine_import_dll: None,
        wine_iat_rva: None,
        wine_string_rva: None,
        wine_reference: None,
    };

    // Advisory confidence signal: does this function still reach the Wine host
    // query? A negative answer is not proof of absence -- the call may be one
    // level indirect -- so it is reported, never used to block.
    if let Ok(Some((dll, iat_rva))) = image.find_import(WINE_IMPORT) {
        site.wine_import_dll = Some(dll);
        site.wine_iat_rva = Some(iat_rva);
    }
    trace_wine_reference(&image, export.rva, &mut site);

    info.site = Some(site);
    info
}

/// Try to show that this function still reaches `wine_get_host_version`.
///
/// Three shapes are recognised, because the real client uses the third:
///
///   1. a RIP-relative reference to the import's IAT slot inside the function
///   2. a `call` into a one-instruction `jmp qword [rip+disp32]` import thunk
///   3. a RIP-relative `lea` of the literal string "wine_get_host_version",
///      which the function then hands to GetProcAddress
///
/// Shape 3 is what World of Warships actually does. It cannot statically
/// import a Wine-only symbol -- the DLL would fail to load on real Windows --
/// so it resolves it at runtime instead.
///
/// A negative result is never proof of absence, so this only ever produces a
/// note. It never blocks a patch.
fn trace_wine_reference(image: &pe::PEFile, function_rva: u32, site: &mut Site) {
    let boundary = image.next_export_rva_after(function_rva).ok().flatten();
    let window = match boundary {
        None => 512usize,
        Some(boundary) => {
            let span = boundary.saturating_sub(function_rva) as usize;
            span.clamp(16, 512)
        }
    };

    if let Some(iat_rva) = site.wine_iat_rva {
        let direct = image
            .find_rip_references(function_rva, iat_rva, window)
            .unwrap_or_default();
        if let Some(reference) = direct.first() {
            site.wine_reference = Some(format!(
                "direct {} at RVA 0x{:X}",
                reference.mnemonic, reference.rva
            ));
            return;
        }

        let calls = image
            .find_relative_calls(function_rva, window)
            .unwrap_or_default();
        for (call_site, target) in calls {
            if image.is_import_thunk_for(target, iat_rva) {
                site.wine_reference = Some(format!(
                    "call at RVA 0x{:X} -> import thunk at 0x{:X}",
                    call_site, target
                ));
                return;
            }
        }
    }

    for string_rva in image.find_ascii_string(WINE_IMPORT) {
        let Ok(references) = image.find_rip_references(function_rva, string_rva, window) else {
            continue;
        };
        if let Some(reference) = references.first() {
            site.wine_string_rva = Some(string_rva);
            site.wine_reference = Some(format!(
                "{} at RVA 0x{:X} loads the \"{}\" string at 0x{:X} (resolved at runtime)",
                reference.mnemonic, reference.rva, WINE_IMPORT, string_rva
            ));
            return;
        }
    }
}

// Classification -----------------------------------------------------------

/// Decide the state, and explain why.
pub fn classify(target: &DllInfo, backup: &DllInfo, record: Option<&Value>) -> (String, Vec<String>) {
    let mut notes: Vec<String> = Vec::new();

    if !target.exists {
        return (
            UNRESOLVED.to_string(),
            vec!["DLL 不在预期路径上".to_string()],
        );
    }
    if target.error.is_some() || target.site.is_none() {
        return (
            UNRESOLVED.to_string(),
            vec![target
                .error
                .clone()
                .unwrap_or_else(|| "无法定位补丁位置".to_string())],
        );
    }
    let site = target.site.as_ref().expect("checked above");

    if !site.section_is_code {
        notes.push(format!(
            "该导出落在节 '{}' 内，而这一节没有被标记为可执行代码",
            site.section
        ));
        return (FOREIGN.to_string(), notes);
    }
    if !site.references_wine() {
        notes.push(format!(
            "在这个函数里看不到对 {} 的直接引用（也可能是间接调用；这只是提示）",
            WINE_IMPORT
        ));
    }

    let backup_ok = backup.exists
        && backup.error.is_none()
        && backup
            .site
            .as_ref()
            .is_some_and(|backup_site| !backup_site.is_patched());
    if backup.exists && !backup_ok {
        notes.push(format!(
            "存在备份，但它看起来不是干净的原件（{}）",
            backup
                .error
                .clone()
                .unwrap_or_else(|| "它的补丁位置已经是打过补丁的字节".to_string())
        ));
    }

    if site.is_patched() {
        if !backup.exists {
            notes.push(format!(
                "补丁已生效，但 DLL 旁边没有备份，本工具无法还原它。\
                 请用 {} 重新获取一份干净的文件。",
                REPAIR_HINT
            ));
            return (FOREIGN.to_string(), notes);
        }
        if !backup_ok {
            notes.push("无法验证的备份不会被当作可还原的来源".to_string());
            return (FOREIGN.to_string(), notes);
        }
        if let Some(record) = record {
            let recorded = record.get("sha256_original").and_then(Value::as_str);
            if recorded.is_some_and(|recorded| recorded != backup.sha256) {
                notes.push(
                    "备份的哈希与该构建记录的原件哈希不一致，它可能属于另一个游戏版本"
                        .to_string(),
                );
                return (FOREIGN.to_string(), notes);
            }
        }
        if record.is_none() {
            notes.push(
                "该 DLL 没有清单记录，但备份已验证为干净原件，因此仍可还原".to_string(),
            );
        }
        return (PATCHED.to_string(), notes);
    }

    // Not patched.
    if backup.exists && backup.sha256 != target.sha256 {
        notes.push(
            "存在来自旧构建的过期备份，它将被当前 DLL 的备份替换".to_string(),
        );
    }
    (ORIGINAL.to_string(), notes)
}

/// Full read-only assessment of an install.
pub fn resolve(install: Install, manifest: Option<&Manifest>) -> Report {
    resolve_with(install, manifest, &mut |_| {}, &AtomicBool::new(false))
}

/// `resolve`, but the sidecar walk reports progress and can be cancelled.
///
/// The GUI needs this: the walk covers an entire game install, which on an
/// external drive is slow enough that a frozen window looks like a hang.
pub fn resolve_with(
    install: Install,
    manifest: Option<&Manifest>,
    on_scan: &mut dyn FnMut(appledouble::ScanProgress),
    cancel: &AtomicBool,
) -> Report {
    // Work off a local clone so the report can be built without holding a
    // borrow on itself.
    let sidecars =
        appledouble::scan_with_progress(&install.root, install.build.as_deref(), on_scan, cancel);

    let mut report = Report {
        install: Some(install.clone()),
        target: None,
        backup: None,
        record: None,
        sidecars: Some(sidecars),
        state: UNRESOLVED.to_string(),
        notes: Vec::new(),
        error: None,
    };

    let Some(build) = install.build.clone() else {
        report.error = Some(format!(
            "{} 下没有编号形式的构建目录（bin/）",
            install.root.display()
        ));
        report.state = UNRESOLVED.to_string();
        return report;
    };

    let (Some(dll), Some(backup_path)) = (install.dll(), install.backup()) else {
        report.error = Some(format!("构建 {} 的 DLL 路径无法确定", build));
        return report;
    };

    report.target = Some(inspect_dll(&dll));
    report.backup = Some(inspect_dll(&backup_path));
    report.record = manifest.and_then(|manifest| manifest.record_for(&dll));

    if install.has_stale_builds() {
        report.notes.push(format!(
            "bin/ 下有 {} 个构建目录；本次针对构建 {}（来源：{}）",
            install.available_builds.len(),
            build,
            install.build_source
        ));
    }
    let incomplete = install.incomplete_builds();
    if !incomplete.is_empty() {
        report.notes.push(format!(
            "已忽略 {} 个缺少 bin64/{} 的构建目录：{}",
            incomplete.len(),
            locate::DLL_NAME,
            incomplete.join(", ")
        ));
    }

    let (state, notes) = classify(
        report.target.as_ref().expect("just set"),
        report.backup.as_ref().expect("just set"),
        report.record.as_ref(),
    );
    report.state = state;
    report.notes.extend(notes);

    let blocking = report
        .sidecars
        .as_ref()
        .map(Scan::in_live_build)
        .unwrap_or_default();
    if !blocking.is_empty() {
        let example = blocking
            .iter()
            .find(|path| path.extension().is_some_and(|ext| ext == "idx"))
            .unwrap_or(&blocking[0]);
        let example_name = example
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        report.notes.push(format!(
            "{0} 个 '._' 文件位于 bin/{1}/ 内。exFAT 把 macOS 的文件元数据存放在这些文件里，\
             CrossOver 会把它们当成真实文件交给游戏：客户端会尝试把 '{2}' 当作资源索引加载，\
             并以 \"{3}\" 崩溃。它们不含任何游戏数据，'clean' 命令会删除它们。",
            blocking.len(),
            build,
            example_name,
            appledouble::SYMPTOM
        ));
    }

    report
}

// Mutation -----------------------------------------------------------------

/// Guard against the external drive vanishing between check and write.
fn require_volume(path: &Path) -> Result<(), PatchError> {
    let parent = path.parent().unwrap_or(Path::new("/"));
    if !parent.is_dir() {
        return Err(PatchError(format!(
            "{} 不可访问 —— 磁盘是否还挂载着？",
            parent.display()
        )));
    }
    Ok(())
}

/// Write via a temp file in the same directory, then rename over.
///
/// A seek-and-write in place would leave a half-written DLL if an external
/// drive hiccups mid-operation.
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), PatchError> {
    require_volume(path)?;
    super::fsutil::atomic_write(path, data).map_err(PatchError::from)
}

/// Back up and patch. Caller is responsible for user confirmation.
pub fn apply(report: &Report, manifest: &mut Manifest) -> Result<Value, PatchError> {
    if report.state != ORIGINAL {
        return Err(PatchError(format!(
            "拒绝打补丁：当前状态是 {}（{}）",
            report.state,
            state_help(&report.state)
        )));
    }

    let target = report
        .target
        .as_ref()
        .ok_or_else(|| PatchError("缺少目标 DLL 信息".to_string()))?;
    let install = report
        .install
        .as_ref()
        .ok_or_else(|| PatchError("缺少安装信息".to_string()))?;
    let site = target
        .site
        .as_ref()
        .ok_or_else(|| PatchError("无法定位补丁位置".to_string()))?;

    let dll = target.path.clone();
    let backup_path = install
        .backup()
        .ok_or_else(|| PatchError("备份路径无法确定".to_string()))?;
    let original_bytes = site.current_bytes.clone();

    // Invariant that protects the backup: we only ever copy a file whose patch
    // site does NOT already hold our bytes. That is what stops a second run
    // from overwriting a good backup with an already-patched DLL.
    if original_bytes == PATCH_BYTES {
        return Err(PatchError(
            "内部校验失败：该 DLL 已经打过补丁".to_string(),
        ));
    }

    require_volume(&dll)?;
    let current_sha = sha256_of(&dll)?;
    if current_sha != target.sha256 {
        return Err(PatchError(
            "DLL 在检查之后发生了变化；请重新运行状态检查".to_string(),
        ));
    }

    let mut replaced_backup: Option<String> = None;
    if backup_path.is_file() {
        let existing = sha256_of(&backup_path)?;
        if existing != current_sha {
            replaced_backup = Some(existing);
        }
    }
    std::fs::copy(&dll, &backup_path)?;

    let mut data = std::fs::read(&dll)?;
    let start = site.file_offset;
    let end = (start + PATCH_BYTES.len()).min(data.len());
    if start >= data.len() {
        return Err(PatchError(format!(
            "补丁偏移 0x{:X} 越过了文件末尾",
            start
        )));
    }
    data[start..end].copy_from_slice(&PATCH_BYTES[..end - start]);
    atomic_write(&dll, &data)?;

    appledouble::drop_sidecar(&backup_path);
    appledouble::drop_sidecar(&dll);

    let patched_sha = sha256_of(&dll)?;
    let record = json!({
        "install": install.root.display().to_string(),
        "build": install.build,
        "version": install.version,
        "dll": dll.display().to_string(),
        "backup": backup_path.display().to_string(),
        "export": site.export_name,
        "export_rva": site.export_rva,
        "file_offset": site.file_offset,
        "original_bytes": hexbytes(&original_bytes),
        "patched_bytes": hexbytes(&PATCH_BYTES),
        "sha256_original": current_sha,
        "sha256_patched": patched_sha,
        "size": target.size,
        "replaced_stale_backup_sha256": replaced_backup,
        "patched_at": super::manifest::now(),
    });

    manifest.put(record.clone());
    manifest.remember_install(&install.root);
    manifest.save()?;
    Ok(record)
}

/// Copy the backup back over the DLL.
pub fn restore(report: &Report, manifest: &mut Manifest) -> Result<Value, PatchError> {
    let target = report
        .target
        .as_ref()
        .ok_or_else(|| PatchError("缺少目标 DLL 信息".to_string()))?;
    let backup = report
        .backup
        .as_ref()
        .ok_or_else(|| PatchError("缺少备份信息".to_string()))?;

    if !backup.exists {
        return Err(PatchError(
            "DLL 旁边没有可用于还原的备份".to_string(),
        ));
    }
    let Some(backup_site) = backup.site.as_ref() else {
        return Err(PatchError(format!(
            "备份不是可读的 PE 映像：{}",
            backup.error.clone().unwrap_or_else(|| "未知".to_string())
        )));
    };
    if backup_site.is_patched() {
        return Err(PatchError(format!(
            "备份本身就已经是打过补丁的 —— 用它还原不会改变任何东西。\
             请改用 {}。",
            REPAIR_HINT
        )));
    }

    if let Some(record) = report.record.as_ref() {
        let recorded = record.get("sha256_original").and_then(Value::as_str);
        if recorded.is_some_and(|recorded| recorded != backup.sha256) {
            return Err(PatchError(
                "备份的 SHA-256 与该构建记录的原件不一致。拒绝用来自其他游戏版本的文件还原。"
                    .to_string(),
            ));
        }
    }

    require_volume(&target.path)?;
    let bytes = std::fs::read(&backup.path)?;
    atomic_write(&target.path, &bytes)?;
    appledouble::drop_sidecar(&target.path);

    manifest.mark_restored(&target.path);
    manifest.save()?;

    Ok(json!({
        "dll": target.path.display().to_string(),
        "backup": backup.path.display().to_string(),
        "sha256_restored": sha256_of(&target.path)?,
        "restored_at": super::manifest::now(),
    }))
}

/// Delete the install's AppleDouble sidecars. Caller confirms first.
///
/// Re-scans instead of trusting the report, so sidecars created since it was
/// taken (by `apply`, or a launcher still writing) are caught too.
pub fn clean(report: &mut Report) -> Result<Value, PatchError> {
    let (root, build) = report
        .install
        .as_ref()
        .map(|install| (install.root.clone(), install.build.clone()))
        .ok_or_else(|| PatchError("缺少安装信息".to_string()))?;

    require_volume(&root.join("bin"))?;

    let found = appledouble::scan(&root, build.as_deref());
    let failures = appledouble::remove(&found.sidecars);

    report.sidecars = Some(appledouble::scan(&root, build.as_deref()));

    Ok(json!({
        "found": found.count(),
        "removed": found.count() - failures.len(),
        "failures": failures,
    }))
}

/// Pick an install from an explicit path, the manifest, or auto-detection.
pub fn resolve_install(path: Option<&str>, manifest: Option<&Manifest>) -> Result<Install, PatchError> {
    if let Some(path) = path.filter(|path| !path.is_empty()) {
        return locate::from_path(Path::new(path)).map_err(|exc| PatchError(exc.to_string()));
    }

    if let Some(remembered) = manifest.and_then(Manifest::last_install) {
        let remembered = PathBuf::from(remembered);
        if locate::looks_like_install(&remembered) {
            return Ok(locate::inspect(&remembered));
        }
    }

    let found = locate::discover();
    if found.is_empty() {
        return Err(PatchError(
            "找不到《战舰世界》的安装目录。请手动指定一个：\n  \
             --install \"/Volumes/YourDrive/World_of_Warships\""
                .to_string(),
        ));
    }
    if found.len() > 1 {
        let listing = found
            .iter()
            .map(|item| format!("  {}", item.root.display()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(PatchError(format!(
            "找到 {} 个安装，请用 --install 指定其中一个：\n{}",
            found.len(),
            listing
        )));
    }
    Ok(found.into_iter().next().expect("length checked above"))
}
