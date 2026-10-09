//! Run the patch engine against a real install and print a status report.
//!
//! Validates the PE parser, export lookup, RVA translation, RIP-relative scan,
//! sidecar walk and state machine against a real Wargaming binary rather than
//! a synthetic fixture.
//!
//!     cargo run --example inspect --offline -- "/path/to/World of Warships"

use wows_mac_patcher_lib::patcher::{dto::ReportDto, manifest::Manifest, patch};

fn line(label: &str, value: impl std::fmt::Display) {
    println!("  {:<11}{}", label, value);
}

fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(character);
    }
    out
}

fn main() {
    let install_path = std::env::args().nth(1);

    let manifest = Manifest::new();
    println!("  manifest   {}", manifest.path.display());

    let install = match patch::resolve_install(install_path.as_deref(), Some(&manifest)) {
        Ok(install) => install,
        Err(exc) => {
            eprintln!("  {exc}");
            std::process::exit(3);
        }
    };

    let report = patch::resolve(install, Some(&manifest));
    let dto = ReportDto::from(&report);

    println!();
    if let Some(install) = &dto.install {
        line("Install", &install.root);
        line(
            "Build",
            format!(
                "{}   ({})",
                install.version.clone().unwrap_or_else(|| "?".into()),
                install.build_source
            ),
        );
        line(
            "Builds",
            format!(
                "available={:?} patchable={:?} incomplete={:?}",
                install.available_builds, install.patchable_builds, install.incomplete_builds
            ),
        );
    }
    if let Some(target) = &dto.target {
        line("Target", target.path.rsplit('/').next().unwrap_or(&target.path));
        if target.exists {
            let digest = &target.sha256;
            line(
                "SHA-256",
                format!(
                    "{}...{}   ({} bytes)",
                    &digest[..digest.len().min(6)],
                    &digest[digest.len().saturating_sub(4)..],
                    thousands(target.size)
                ),
            );
        } else {
            line("", "file not found");
        }
        if let Some(site) = &target.site {
            println!();
            line("Export", &site.export_name);
            line(
                "Address",
                format!(
                    "RVA 0x{:08X}  ->  file offset 0x{:X}",
                    site.export_rva, site.file_offset
                ),
            );
            line(
                "Section",
                format!(
                    "{}{}",
                    site.section,
                    if site.section_is_code {
                        "   executable code"
                    } else {
                        "   NOT CODE"
                    }
                ),
            );
            line("Bytes", &site.current_bytes);
            line("Wine path", site.wine_reference.as_deref().unwrap_or("(none)"));
            line("Wine iat", format!("{:?}", site.wine_iat_rva));
            line("Wine str", format!("{:?}", site.wine_string_rva));
        } else if let Some(error) = &target.error {
            line("Export", "UNRESOLVED");
            line("", error);
        }
    }

    println!();
    match &dto.backup {
        Some(backup) if backup.exists => {
            let digest = &backup.sha256;
            line(
                "Backup",
                format!(
                    "{}   {}...{}",
                    backup.name,
                    &digest[..digest.len().min(6)],
                    &digest[digest.len().saturating_sub(4)..]
                ),
            );
        }
        _ => line("Backup", "none"),
    }

    line("State", format!("{} - {}", dto.state, dto.state_help));
    line(
        "Patchable",
        format!(
            "can_patch={} can_restore={} needs_clean={}",
            dto.can_patch, dto.can_restore, dto.needs_clean
        ),
    );
    match &dto.sidecars {
        Some(scan) if scan.count == 0 => line("Sidecars", "none"),
        Some(scan) => {
            line("Sidecars", format!("{} files", scan.count));
            line("in build", scan.in_live_build);
        }
        None => {}
    }

    if let Some(error) = &dto.error {
        println!();
        line("Error", error);
    }
    for note in &dto.notes {
        println!();
        line("Note", note);
    }
    println!();
}
