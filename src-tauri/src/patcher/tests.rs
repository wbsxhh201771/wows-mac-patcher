//! Regression tests for the patch engine.
//!
//! Covers PE parsing, the four-state machine, backup invariants, and
//! AppleDouble cleanup. The PE fixture is synthesised in pure Rust
//! (`fixture::platform64_dll`) — no mingw / external DLL required.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::{appledouble, fixture, locate, manifest::Manifest, patch, pe};

const LIVE_BUILD: &str = "13015811";
const STALE_BUILD: &str = "12000000";

// What macOS actually writes on exFAT for a com.apple.provenance attribute.
fn sidecar_bytes() -> Vec<u8> {
    let mut bytes = appledouble::MAGIC.to_vec();
    bytes.extend_from_slice(b"\x00\x02\x00\x00Mac OS X        ");
    bytes.extend_from_slice(&[0u8; 64]);
    bytes
}

fn pak_index_bytes() -> Vec<u8> {
    let mut bytes = b"ISFP\x00\x00\x00\x02".to_vec();
    bytes.extend_from_slice(&[0u8; 24]);
    bytes
}

/// Minimal stand-in for `tempfile::TempDir`, which this crate does not depend
/// on. Removes the tree on drop, so a failing assertion still cleans up.
struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);

        let path = std::env::temp_dir().join(format!(
            "{}{}-{}",
            prefix,
            std::process::id(),
            sequence
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        TestDir { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn game_info() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<protocol version="1.9">
  <game>
    <id>WOWS.WW.PRODUCTION</id>
    <index>WOWS.WW.PRODUCTION</index>
    <parts>
      <part>
        <name>client</name>
        <version>15.7.0.{}</version>
      </part>
    </parts>
  </game>
</protocol>
"#,
        LIVE_BUILD
    )
}

/// A synthetic install with a live build, a stale build, and a manifest.
struct Install {
    _temp: TestDir,
    root: PathBuf,
    manifest: Manifest,
}

impl Install {
    fn new() -> Self {
        let temp = TestDir::new("wows-test-");
        let root = temp.path().join("World_of_Warships");
        let dll = fixture::platform64_dll();

        for build in [LIVE_BUILD, STALE_BUILD] {
            let target = root.join("bin").join(build).join("bin64");
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join("platform64.dll"), &dll).unwrap();
        }
        std::fs::write(root.join("game_info.xml"), game_info()).unwrap();

        let manifest = Manifest::with_path(temp.path().join("state.json"));
        Install {
            _temp: temp,
            root,
            manifest,
        }
    }

    fn resolve(&self) -> locate::Install {
        locate::from_path(&self.root).expect("root is a valid install")
    }

    /// Reload the manifest from disk, matching production command behaviour.
    fn report(&self) -> patch::Report {
        let manifest = Manifest::with_path(self.manifest.path.clone());
        patch::resolve(self.resolve(), Some(&manifest))
    }

    fn dll(&self) -> PathBuf {
        self.root
            .join("bin")
            .join(LIVE_BUILD)
            .join("bin64")
            .join("platform64.dll")
    }

    fn backup(&self) -> PathBuf {
        self.dll().parent().unwrap().join("platform64_bck.dll")
    }
}

// =========================================================================
// PE parsing
// =========================================================================

fn fixture_image() -> pe::PEFile {
    pe::PEFile::from_bytes(fixture::platform64_dll(), "fixture").expect("fixture is a PE image")
}

#[test]
fn identifies_a_64_bit_image() {
    let image = fixture_image();

    assert!(image.is_64bit);
    assert_eq!(image.machine, pe::MACHINE_AMD64);
    assert_eq!(image.machine_name(), "x86-64");
}

#[test]
fn rejects_non_pe_data() {
    let result = pe::PEFile::from_bytes(b"this is not a PE file, not even close".to_vec(), "x");
    assert!(result.is_err());
}

#[test]
fn finds_the_mangled_export_exactly_once() {
    let image = fixture_image();

    let matches = image.find_exports("getHostVersionFromWine").unwrap();
    assert_eq!(
        matches.len(),
        1,
        "{:?}",
        matches.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
    assert!(matches[0].name.contains("getHostVersionFromWine"));
    assert!(!matches[0].is_forwarder());
}

#[test]
fn decoy_exports_do_not_collide() {
    let image = fixture_image();

    // getHostName shares a prefix but must not match the needle.
    let names: Vec<String> = image.exports().unwrap().into_iter().map(|e| e.name).collect();
    assert!(names.iter().any(|name| name.contains("getHostName")));
    assert_eq!(image.find_exports("getHostVersionFromWine").unwrap().len(), 1);
}

#[test]
fn export_resolves_into_an_executable_section() {
    let image = fixture_image();

    let export = image.find_exports("getHostVersionFromWine").unwrap().remove(0);
    let section = image.section_for_rva(export.rva).expect("export has a section");
    assert!(section.is_executable_code(), "{}", section.name);
}

#[test]
fn rva_to_offset_round_trips_against_raw_bytes() {
    let image = fixture_image();

    let export = image.find_exports("getHostVersionFromWine").unwrap().remove(0);
    let offset = image.rva_to_offset(export.rva).unwrap();
    assert!(offset > 0);
    assert!(offset < image.data.len());
    assert_eq!(
        &image.data[offset..offset + 8],
        image.read_at_rva(export.rva, 8).unwrap().as_slice()
    );
}

#[test]
fn rva_outside_every_section_is_rejected() {
    let image = fixture_image();
    assert!(image.rva_to_offset(0x7FFF_FFFF).is_err());
}

#[test]
fn locates_the_wine_import() {
    let image = fixture_image();

    let (dll, iat_rva) = image
        .find_import("wine_get_host_version")
        .unwrap()
        .expect("the fixture imports ntdll's wine_get_host_version");
    assert_eq!(dll.to_lowercase(), "ntdll.dll");
    assert!(iat_rva > 0);
}

#[test]
fn reaches_the_wine_import_through_a_thunk() {
    let image = fixture_image();

    // Compilers route imported calls through a one-instruction thunk, so the
    // function usually holds no direct reference to the IAT slot.
    let export = image.find_exports("getHostVersionFromWine").unwrap().remove(0);
    let (_dll, iat_rva) = image.find_import("wine_get_host_version").unwrap().unwrap();

    let thunked: Vec<_> = image
        .find_relative_calls(export.rva, 128)
        .unwrap()
        .into_iter()
        .filter(|(_site, target)| image.is_import_thunk_for(*target, iat_rva))
        .collect();
    assert!(
        !thunked.is_empty(),
        "expected a call reaching the wine import thunk"
    );
}

#[test]
fn unrelated_function_does_not_reach_the_wine_import() {
    let image = fixture_image();

    let export = image.find_exports("computeSomethingElse").unwrap().remove(0);
    let (_dll, iat_rva) = image.find_import("wine_get_host_version").unwrap().unwrap();

    assert!(image
        .find_rip_references(export.rva, iat_rva, 32)
        .unwrap()
        .is_empty());

    let thunked: Vec<_> = image
        .find_relative_calls(export.rva, 32)
        .unwrap()
        .into_iter()
        .filter(|(_site, target)| image.is_import_thunk_for(*target, iat_rva))
        .collect();
    assert!(thunked.is_empty());
}

#[test]
fn finds_the_wine_symbol_as_a_literal_string() {
    let image = fixture_image();

    let hits = image.find_ascii_string("wine_get_host_version");
    assert!(!hits.is_empty());
    for rva in hits {
        assert_eq!(
            image.read_at_rva(rva, 21).unwrap(),
            b"wine_get_host_version".to_vec()
        );
    }
}

#[test]
fn offset_to_rva_round_trips() {
    let image = fixture_image();

    let export = image.find_exports("getHostVersionFromWine").unwrap().remove(0);
    let offset = image.rva_to_offset(export.rva).unwrap();
    assert_eq!(image.offset_to_rva(offset as u32), Some(export.rva));
}

#[test]
fn runtime_resolved_function_references_the_string() {
    let image = fixture_image();

    // This is how the shipping DLL reaches Wine: lea the symbol name, then
    // hand it to GetProcAddress. There is no IAT slot to point at.
    let export = image
        .find_exports("getHostVersionViaProcAddress")
        .unwrap()
        .remove(0);

    let referenced: Vec<u32> = image
        .find_ascii_string("wine_get_host_version")
        .into_iter()
        .filter(|rva| {
            image
                .find_rip_references(export.rva, *rva, 256)
                .map(|hits| !hits.is_empty())
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !referenced.is_empty(),
        "expected a lea of the symbol-name string"
    );
}

#[test]
fn thunk_detector_rejects_a_non_thunk() {
    let image = fixture_image();

    let export = image.find_exports("getHostVersionFromWine").unwrap().remove(0);
    let (_dll, iat_rva) = image.find_import("wine_get_host_version").unwrap().unwrap();
    // The function body itself is not a bare jmp-to-IAT thunk.
    assert!(!image.is_import_thunk_for(export.rva as i64, iat_rva));
}

// =========================================================================
// AppleDouble sidecars
// =========================================================================

/// A synthetic install tree populated with real files and sidecars.
struct SidecarFixture {
    _temp: TestDir,
    root: PathBuf,
    build: PathBuf,
    real_files: Vec<PathBuf>,
    sidecars: Vec<PathBuf>,
}

impl SidecarFixture {
    fn new() -> Self {
        let temp = TestDir::new("wows-appledouble-");
        let root = temp.path().join("World of Warships");
        let build = root.join("bin").join("13187581");

        for sub in ["bin64", "idx", "res/texts"] {
            std::fs::create_dir_all(build.join(sub)).unwrap();
        }
        std::fs::create_dir_all(root.join("res_packages")).unwrap();

        let real_files = vec![
            build.join("idx").join("basecontent.idx"),
            build.join("bin64").join("paths.xml"),
            root.join("res_packages").join("basecontent_0001.pkg"),
        ];
        for path in &real_files {
            std::fs::write(path, pak_index_bytes()).unwrap();
        }

        let sidecars = vec![
            build.join("idx").join("._basecontent.idx"),
            build.join("res").join("texts").join("._en"),
            root.join("res_packages").join("._basecontent_0001.pkg"),
            root.join("._WorldOfWarships.exe"),
        ];
        for path in &sidecars {
            std::fs::write(path, sidecar_bytes()).unwrap();
        }

        SidecarFixture {
            _temp: temp,
            root,
            build,
            real_files,
            sidecars,
        }
    }

    fn live_build(&self) -> &str {
        "13187581"
    }

    fn manifest(&self) -> Manifest {
        Manifest::with_path(self._temp.path().join("state.json"))
    }

    fn report(&self) -> patch::Report {
        let install = locate::from_path(&self.root).expect("valid install");
        patch::resolve(install, Some(&self.manifest()))
    }
}

fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths
}

#[test]
fn sidecar_needs_both_the_name_and_the_magic() {
    let fixture = SidecarFixture::new();

    assert!(appledouble::is_sidecar(&fixture.sidecars[0]));

    let wrong_content = fixture.build.join("idx").join("._not_appledouble.idx");
    std::fs::write(&wrong_content, pak_index_bytes()).unwrap();
    assert!(!appledouble::is_sidecar(&wrong_content));

    let wrong_name = fixture.build.join("idx").join("looks_like_appledouble.idx");
    std::fs::write(&wrong_name, sidecar_bytes()).unwrap();
    assert!(!appledouble::is_sidecar(&wrong_name));
}

#[test]
fn directories_and_symlinks_are_never_sidecars() {
    let fixture = SidecarFixture::new();

    let directory = fixture.build.join("._dir");
    std::fs::create_dir_all(&directory).unwrap();
    assert!(!appledouble::is_sidecar(&directory));

    let link = fixture.build.join("._link");
    std::os::unix::fs::symlink(&fixture.sidecars[0], &link).unwrap();
    assert!(!appledouble::is_sidecar(&link));
}

#[test]
fn scan_finds_every_sidecar_in_the_install() {
    let fixture = SidecarFixture::new();
    let scan = appledouble::scan(&fixture.root, Some(fixture.live_build()));
    assert_eq!(sorted(scan.sidecars.clone()), sorted(fixture.sidecars.clone()));
}

#[test]
fn only_sidecars_in_the_live_build_are_blocking() {
    let fixture = SidecarFixture::new();
    let scan = appledouble::scan(&fixture.root, Some(fixture.live_build()));
    assert_eq!(
        sorted(scan.in_live_build()),
        sorted(fixture.sidecars[..2].to_vec())
    );
}

#[test]
fn report_explains_the_crash() {
    let fixture = SidecarFixture::new();
    let report = fixture.report();

    assert!(report.needs_clean());
    let note = report
        .notes
        .iter()
        .find(|note| note.contains(appledouble::SYMPTOM))
        .expect("a note carries the verbatim crash text");
    assert!(note.contains("._basecontent.idx"));
}

#[test]
fn remove_deletes_sidecars_and_nothing_else() {
    let fixture = SidecarFixture::new();
    let impostor = fixture.build.join("idx").join("._not_appledouble.idx");
    std::fs::write(&impostor, pak_index_bytes()).unwrap();

    let mut targets = fixture.sidecars.clone();
    targets.push(impostor.clone());
    targets.extend(fixture.real_files.clone());

    let failures = appledouble::remove(&targets);
    assert!(failures.is_empty());

    for path in &fixture.sidecars {
        assert!(!path.exists(), "{}", path.display());
    }
    for path in fixture.real_files.iter().chain(std::iter::once(&impostor)) {
        assert!(path.exists(), "{}", path.display());
    }
}

#[test]
fn drop_sidecar_removes_only_the_named_files_sidecar() {
    let fixture = SidecarFixture::new();
    appledouble::drop_sidecar(&fixture.real_files[0]);

    assert!(!fixture.sidecars[0].exists());
    assert!(fixture.real_files[0].exists());
    assert!(fixture.sidecars[1].exists());
}

#[test]
fn drop_sidecar_without_a_sidecar_is_a_no_op() {
    let fixture = SidecarFixture::new();
    appledouble::drop_sidecar(&fixture.real_files[1]);
    assert!(fixture.real_files[1].exists());
}

#[test]
fn clean_rescans_and_reports() {
    let fixture = SidecarFixture::new();
    let mut report = fixture.report();

    // Appeared after the report was taken.
    let late = fixture.build.join("bin64").join("._platform64.dll");
    std::fs::write(&late, sidecar_bytes()).unwrap();

    let result = patch::clean(&mut report).unwrap();

    assert_eq!(result["found"], json!(fixture.sidecars.len() + 1));
    assert_eq!(result["removed"], json!(fixture.sidecars.len() + 1));
    assert!(!late.exists());
    assert!(!report.needs_clean());
}

// =========================================================================
// Install locate
// =========================================================================

#[test]
fn resolves_live_build_from_game_info() {
    let install = Install::new().resolve();

    assert_eq!(install.build.as_deref(), Some(LIVE_BUILD));
    assert_eq!(install.build_source, "game_info.xml");
    assert_eq!(install.version.as_deref(), Some(&*format!("15.7.0.{}", LIVE_BUILD)));
}

#[test]
fn does_not_pick_the_stale_build() {
    let install = Install::new().resolve();

    assert!(install.available_builds.contains(&STALE_BUILD.to_string()));
    assert!(!install
        .dll()
        .expect("a build was resolved")
        .display()
        .to_string()
        .contains(STALE_BUILD));
}

#[test]
fn falls_back_to_highest_build_without_game_info() {
    let install = Install::new();
    // Steam installs ship no game_info.xml, so this is their normal path.
    std::fs::remove_file(install.root.join("game_info.xml")).unwrap();

    let resolved = install.resolve();
    // 13015811 > 12000000
    assert_eq!(resolved.build.as_deref(), Some(LIVE_BUILD));
    assert_eq!(
        resolved.build_source,
        "highest-numbered bin/ directory with the DLL"
    );
}

#[test]
fn ignores_build_directories_without_the_dll() {
    let install = Install::new();
    // Updates leave behind build dirs that hold no bin64/platform64.dll.
    // Picking one produces a baffling "file not found" instead of a patch.
    std::fs::remove_file(install.root.join("game_info.xml")).unwrap();
    std::fs::create_dir_all(install.root.join("bin").join("99999999").join("bin64")).unwrap();

    let resolved = install.resolve();
    assert!(resolved.available_builds.contains(&"99999999".to_string()));
    assert!(!resolved.patchable_builds.contains(&"99999999".to_string()));
    assert_eq!(resolved.incomplete_builds(), vec!["99999999".to_string()]);
    assert_eq!(resolved.build.as_deref(), Some(LIVE_BUILD));

    let report = patch::resolve(resolved, Some(&install.manifest));
    assert_eq!(report.state, patch::ORIGINAL);
    assert!(report.notes.iter().any(|note| note.contains("99999999")));
}

#[test]
fn matches_steam_style_folder_names() {
    let install = Install::new();

    for name in ["World of Warships", "World_of_Warships", "WorldOfWarships PT"] {
        let renamed = install._temp.path().join(name);
        if renamed.exists() {
            continue;
        }
        std::fs::rename(&install.root, &renamed).unwrap();

        assert!(locate::looks_like_install(&renamed), "{}", name);
        assert_eq!(locate::from_path(&renamed).unwrap().root, renamed);

        std::fs::rename(&renamed, &install.root).unwrap();
    }
}

#[test]
fn does_not_match_world_of_warplanes() {
    let install = Install::new();

    std::fs::create_dir_all(install._temp.path().join("World of Warplanes")).unwrap();
    let names: Vec<String> = locate::children_named_like_install(install._temp.path())
        .into_iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    assert!(!names.contains(&"World of Warplanes".to_string()));
    assert!(names.contains(&"World_of_Warships".to_string()));
}

#[test]
fn accepts_a_path_inside_the_install() {
    let install = Install::new();
    let resolved = locate::from_path(&install.dll()).unwrap();
    assert_eq!(resolved.root, install.root);
}

#[test]
fn rejects_an_unrelated_directory() {
    let install = Install::new();
    assert!(locate::from_path(install._temp.path()).is_err());
}

// =========================================================================
// Patch state machine
// =========================================================================

#[test]
fn clean_install_is_original() {
    let install = Install::new();
    let report = install.report();

    assert_eq!(report.state, patch::ORIGINAL);
    assert!(report.can_patch());
    assert!(!report.can_restore());
}

#[test]
fn warns_about_multiple_build_directories() {
    let install = Install::new();
    let report = install.report();

    assert!(
        report.notes.iter().any(|note| note.contains("构建目录")),
        "{:?}",
        report.notes
    );
}

#[test]
fn patch_then_status_reports_patched() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    patch::apply(&install.report(), &mut manifest).unwrap();

    let report = install.report();
    assert_eq!(report.state, patch::PATCHED);
    assert!(report.can_restore());
    assert!(!report.can_patch());
}

#[test]
fn patch_writes_exactly_three_bytes() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    let before = std::fs::read(install.dll()).unwrap();
    let record = patch::apply(&install.report(), &mut manifest).unwrap();
    let after = std::fs::read(install.dll()).unwrap();

    assert_eq!(before.len(), after.len());
    let differing: Vec<usize> = (0..before.len()).filter(|&i| before[i] != after[i]).collect();
    assert_eq!(differing.len(), 3, "{:?}", differing);

    let offset = record["file_offset"].as_u64().unwrap() as usize;
    assert_eq!(differing[0], offset);
    assert_eq!(&after[offset..offset + 3], &patch::PATCH_BYTES);
}

#[test]
fn patch_is_refused_twice() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    patch::apply(&install.report(), &mut manifest).unwrap();
    assert!(patch::apply(&install.report(), &mut manifest).is_err());
}

#[test]
fn second_patch_does_not_clobber_the_backup() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    let original = std::fs::read(install.dll()).unwrap();
    patch::apply(&install.report(), &mut manifest).unwrap();
    let _ = patch::apply(&install.report(), &mut manifest);

    assert_eq!(std::fs::read(install.backup()).unwrap(), original);
}

#[test]
fn restore_returns_the_exact_original_bytes() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    let original = std::fs::read(install.dll()).unwrap();
    patch::apply(&install.report(), &mut manifest).unwrap();
    assert_ne!(std::fs::read(install.dll()).unwrap(), original);

    patch::restore(&install.report(), &mut manifest).unwrap();
    assert_eq!(std::fs::read(install.dll()).unwrap(), original);
    assert_eq!(install.report().state, patch::ORIGINAL);
}

#[test]
fn patched_without_backup_is_foreign() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    patch::apply(&install.report(), &mut manifest).unwrap();
    std::fs::remove_file(install.backup()).unwrap();

    let report = install.report();
    assert_eq!(report.state, patch::FOREIGN);
    assert!(!report.can_patch());
    assert!(!report.can_restore());
    assert!(
        report.notes.iter().any(|note| note.contains("检查并修复")),
        "{:?}",
        report.notes
    );
}

#[test]
fn patched_with_a_patched_backup_is_foreign() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    patch::apply(&install.report(), &mut manifest).unwrap();
    // Backup is now patched too.
    std::fs::copy(install.dll(), install.backup()).unwrap();

    assert_eq!(install.report().state, patch::FOREIGN);
}

#[test]
fn restore_refuses_a_mismatched_backup() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    patch::apply(&install.report(), &mut manifest).unwrap();

    let mut report = install.report();
    // Simulate a backup left over from a different game version.
    let mut data = std::fs::read(install.backup()).unwrap();
    let last = data.len() - 1;
    data[last] ^= 0xFF;
    std::fs::write(install.backup(), &data).unwrap();
    report.backup = Some(patch::inspect_dll(&install.backup()));

    assert!(patch::restore(&report, &mut manifest).is_err());
}

#[test]
fn stale_backup_on_unpatched_dll_is_flagged_and_replaced() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    let mut data = std::fs::read(install.dll()).unwrap();
    let last = data.len() - 1;
    data[last] ^= 0xFF;
    std::fs::write(install.backup(), &data).unwrap();

    let report = install.report();
    assert_eq!(report.state, patch::ORIGINAL);
    assert!(
        report.notes.iter().any(|note| note.contains("过期备份")),
        "{:?}",
        report.notes
    );

    let record = patch::apply(&report, &mut manifest).unwrap();
    assert!(!record["replaced_stale_backup_sha256"].is_null());
}

#[test]
fn missing_export_is_unresolved() {
    let install = Install::new();

    // Break the MZ signature so the export cannot be found.
    let mut junk = std::fs::read(install.dll()).unwrap();
    junk[0] = b'X';
    std::fs::write(install.dll(), &junk).unwrap();

    assert_eq!(install.report().state, patch::UNRESOLVED);
}

// =========================================================================
// Manifest
// =========================================================================

#[test]
fn record_captures_the_forensics() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    let record = patch::apply(&install.report(), &mut manifest).unwrap();
    for key in [
        "build",
        "export",
        "file_offset",
        "original_bytes",
        "patched_bytes",
        "sha256_original",
        "sha256_patched",
    ] {
        assert!(record.get(key).is_some(), "missing key {}", key);
    }
    assert_eq!(record["patched_bytes"], json!("31 C0 C3"));
    assert_ne!(record["sha256_original"], record["sha256_patched"]);
}

#[test]
fn manifest_survives_a_reload() {
    let install = Install::new();
    let mut manifest = install.manifest.clone();

    patch::apply(&install.report(), &mut manifest).unwrap();

    let reloaded = Manifest::with_path(install._temp.path().join("state.json"));
    assert!(reloaded.record_for(&install.dll()).is_some());
    assert_eq!(
        reloaded.last_install().as_deref(),
        Some(install.root.display().to_string().as_str())
    );
}

#[test]
fn corrupt_manifest_does_not_block_work() {
    let install = Install::new();

    std::fs::write(install._temp.path().join("state.json"), "{ not json").unwrap();
    let manifest = Manifest::with_path(install._temp.path().join("state.json"));
    assert!(manifest.records().is_empty());
}
