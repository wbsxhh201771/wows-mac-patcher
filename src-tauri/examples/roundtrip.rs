//! End-to-end patch/restore against a *copy* of a real DLL.
//!
//! Unit tests synthesise a PE fixture in Rust. This example instead takes a
//! real `platform64.dll` and builds a throwaway install tree around a copy, so
//! the real game is never touched.
//!
//!     cargo run --example roundtrip --offline -- \
//!       "/path/to/World of Warships/bin/13357625/bin64/platform64.dll"
//!
//! A live install is usually *already patched*, and a patched DLL with no
//! backup beside it is FOREIGN -- correctly un-actionable. So when the given
//! DLL is patched, the clean original next to it (`platform64_bck.dll`) is
//! used as the source instead. That file is exactly the pre-patch bytes, which
//! makes it the honest thing to test against.

use std::path::{Path, PathBuf};

use wows_mac_patcher_lib::patcher::{locate, manifest::Manifest, patch};

const BUILD: &str = "13015811";

/// Pick a clean, unpatched DLL to test with.
fn clean_source(dll: &Path) -> PathBuf {
    let already_patched = patch::inspect_dll(dll)
        .site
        .is_some_and(|site| site.is_patched());
    if !already_patched {
        return dll.to_path_buf();
    }

    let backup = dll.with_file_name(locate::BACKUP_NAME);
    if backup.is_file() {
        println!(
            "源 DLL 已被打过补丁，改用旁边的干净原件 {}",
            locate::BACKUP_NAME
        );
        backup
    } else {
        println!("警告: 源 DLL 已打补丁且没有备份，FOREIGN 判定会拒绝操作");
        dll.to_path_buf()
    }
}

fn main() {
    let Some(argument) = std::env::args().nth(1) else {
        eprintln!("用法: cargo run --example roundtrip -- <platform64.dll 的路径>");
        std::process::exit(2);
    };
    let given = PathBuf::from(argument);
    if !given.is_file() {
        eprintln!("找不到 DLL: {}", given.display());
        std::process::exit(2);
    }
    let dll_path = clean_source(&given);

    // Build a throwaway install tree around a copy of the real binary.
    let sandbox = std::env::temp_dir().join(format!("wows-roundtrip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    let root = sandbox.join("World_of_Warships");
    let bin64 = root.join("bin").join(BUILD).join("bin64");
    std::fs::create_dir_all(&bin64).expect("create sandbox");

    let dll = bin64.join(locate::DLL_NAME);
    std::fs::copy(&dll_path, &dll).expect("copy the real DLL into the sandbox");
    let backup = bin64.join(locate::BACKUP_NAME);
    let manifest_path = sandbox.join("state.json");

    let original = std::fs::read(&dll).expect("read original");
    println!("沙箱      {}", root.display());
    println!("原始大小  {} 字节", original.len());

    let install = locate::from_path(&root).expect("sandbox looks like an install");
    let mut manifest = Manifest::with_path(manifest_path.clone());

    // --- status: must be read-only and report ORIGINAL --------------------
    let report = patch::resolve(install.clone(), Some(&manifest));
    println!("初始状态  {}（can_patch={}）", report.state, report.can_patch());
    assert_eq!(report.state, patch::ORIGINAL, "a fresh copy must be ORIGINAL");
    assert!(report.can_patch());
    assert_eq!(std::fs::read(&dll).unwrap(), original, "status must not write");

    let site = report
        .target
        .as_ref()
        .and_then(|target| target.site.as_ref())
        .expect("the real DLL resolves a patch site");
    println!(
        "补丁位置  {} @ 0x{:X}  当前字节 {}",
        site.export_name,
        site.file_offset,
        patch::hexbytes(&site.current_bytes)
    );

    // --- patch ------------------------------------------------------------
    let record = patch::apply(&report, &mut manifest).expect("patch applies");
    let patched = std::fs::read(&dll).expect("read patched");

    assert_eq!(patched.len(), original.len(), "size must not change");
    let differing: Vec<usize> = (0..original.len())
        .filter(|&index| original[index] != patched[index])
        .collect();
    assert_eq!(differing.len(), 3, "exactly three bytes may change");

    let offset = record["file_offset"].as_u64().expect("offset recorded") as usize;
    assert_eq!(differing[0], offset);
    assert_eq!(&patched[offset..offset + 3], &patch::PATCH_BYTES);
    assert!(backup.is_file(), "a backup must be written");
    assert_eq!(std::fs::read(&backup).unwrap(), original, "backup is original");
    println!("打补丁    写入 3 字节 @ 0x{offset:X}，备份已生成");

    // --- state machine now reports PATCHED --------------------------------
    let install = locate::from_path(&root).expect("re-resolve");
    let report = patch::resolve(install.clone(), Some(&manifest));
    assert_eq!(report.state, patch::PATCHED);
    assert!(report.can_restore());
    println!("补丁后    状态 {}（can_restore={}）", report.state, report.can_restore());

    // --- a second run must be refused, and must not clobber the backup ----
    assert!(
        patch::apply(&report, &mut manifest).is_err(),
        "a second patch must be refused"
    );
    assert_eq!(
        std::fs::read(&backup).unwrap(),
        original,
        "the refused run must not touch the backup"
    );
    println!("重复打补丁 已拒绝，备份未被覆盖");

    // --- manifest round trip ---------------------------------------------
    let reloaded = Manifest::with_path(manifest_path);
    assert!(
        reloaded.record_for(&dll).is_some(),
        "the record must survive a reload"
    );
    println!("清单      记录已落盘并可重新读取");

    // --- restore ----------------------------------------------------------
    let install = locate::from_path(&root).expect("re-resolve");
    let report = patch::resolve(install, Some(&manifest));
    patch::restore(&report, &mut manifest).expect("restore succeeds");
    assert_eq!(
        std::fs::read(&dll).unwrap(),
        original,
        "restore must return the exact original bytes"
    );
    println!("还原      字节与原件完全一致");

    let _ = std::fs::remove_dir_all(&sandbox);
    println!("\n全部通过。");
}
