//! Minimal, dependency-light reader for PE (Portable Executable) images.
//!
//! Only what the patcher needs: the section table, the export directory, the
//! import directory, and RVA -> file-offset translation.
//!
//! Everything here is read-only. Nothing in this module writes to disk.
//!
//! The hand-rolled instruction scans at the bottom (`find_rip_references`,
//! `find_relative_calls`, `is_import_thunk_for`) are deliberately narrow
//! pattern matches, not a disassembler. `patch::classify` reports their
//! results as advisory notes only; their semantics are covered by the test
//! suite and must not drift.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const DOS_SIGNATURE: u16 = 0x5A4D; // 'MZ'
pub const NT_SIGNATURE: u32 = 0x0000_4550; // 'PE\0\0'

pub const MACHINE_AMD64: u16 = 0x8664;
pub const MACHINE_I386: u16 = 0x014C;

pub const MAGIC_PE32: u16 = 0x010B;
pub const MAGIC_PE32PLUS: u16 = 0x020B;

pub const SCN_CNT_CODE: u32 = 0x0000_0020;
pub const SCN_MEM_EXECUTE: u32 = 0x2000_0000;

pub const DIR_EXPORT: usize = 0;
pub const DIR_IMPORT: usize = 1;

/// The file is not a PE image we can reason about safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PEError(pub String);

impl fmt::Display for PEError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PEError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw_data: u32,
    pub pointer_to_raw_data: u32,
    pub characteristics: u32,
}

impl Section {
    pub fn is_executable_code(&self) -> bool {
        self.characteristics & (SCN_CNT_CODE | SCN_MEM_EXECUTE) != 0
    }

    /// A section's in-memory size can exceed its raw size (BSS-like tails).
    /// Use the larger of the two so RVA lookups do not miss.
    pub fn virtual_span(&self) -> u32 {
        self.virtual_size.max(self.size_of_raw_data)
    }

    pub fn contains_rva(&self, rva: u32) -> bool {
        // Widen to u64: virtual_address + virtual_span can legitimately exceed
        // u32 for a section placed at the top of the address space.
        let start = self.virtual_address as u64;
        let end = start + self.virtual_span() as u64;
        (rva as u64) >= start && (rva as u64) < end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub ordinal: u32,
    pub rva: u32,
    pub forwarder: Option<String>,
}

impl Export {
    pub fn is_forwarder(&self) -> bool {
        self.forwarder.is_some()
    }
}

/// A RIP-relative instruction that references some target RVA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RipRef {
    pub rva: u32,
    pub length: usize,
    pub mnemonic: &'static str,
}

/// A parsed PE image held in memory.
#[derive(Debug, Clone)]
pub struct PEFile {
    pub data: Vec<u8>,
    pub source: String,
    pub machine: u16,
    pub timestamp: u32,
    pub is_64bit: bool,
    pub image_base: u64,
    pub directories: Vec<(u32, u32)>,
    pub sections: Vec<Section>,
}

impl PEFile {
    // ------------------------------------------------------------------ //
    // construction
    // ------------------------------------------------------------------ //

    pub fn from_bytes(data: Vec<u8>, source: impl Into<String>) -> Result<Self, PEError> {
        let mut image = PEFile {
            data,
            source: source.into(),
            machine: 0,
            timestamp: 0,
            is_64bit: false,
            image_base: 0,
            directories: Vec::new(),
            sections: Vec::new(),
        };
        image.parse_headers()?;
        Ok(image)
    }

    pub fn from_path(path: &Path) -> Result<Self, PEError> {
        let data = std::fs::read(path)
            .map_err(|exc| PEError(format!("{}: {}", path.display(), exc)))?;
        Self::from_bytes(data, path.display().to_string())
    }

    // ------------------------------------------------------------------ //
    // primitive reads
    // ------------------------------------------------------------------ //

    /// `fmt` is included in the error message for diagnostics.
    fn unpack<const N: usize>(&self, fmt: &str, offset: usize) -> Result<[u8; N], PEError> {
        let slice = offset
            .checked_add(N)
            .and_then(|end| self.data.get(offset..end))
            .ok_or_else(|| {
                PEError(format!(
                    "truncated file: cannot read {} at offset 0x{:x}",
                    fmt, offset
                ))
            })?;
        let mut buffer = [0u8; N];
        buffer.copy_from_slice(slice);
        Ok(buffer)
    }

    fn u16_at(&self, offset: usize) -> Result<u16, PEError> {
        Ok(u16::from_le_bytes(self.unpack::<2>("<H", offset)?))
    }

    fn u32_at(&self, offset: usize) -> Result<u32, PEError> {
        Ok(u32::from_le_bytes(self.unpack::<4>("<I", offset)?))
    }

    fn u64_at(&self, offset: usize) -> Result<u64, PEError> {
        Ok(u64::from_le_bytes(self.unpack::<8>("<Q", offset)?))
    }

    fn cstring(&self, offset: usize, limit: usize) -> Result<String, PEError> {
        let unterminated = || PEError(format!("unterminated string at offset 0x{:x}", offset));
        let end = offset.checked_add(limit).ok_or_else(unterminated)?;
        let end = end.min(self.data.len());
        let window = self.data.get(offset..end).ok_or_else(unterminated)?;
        match window.iter().position(|&byte| byte == 0) {
            Some(nul) => Ok(String::from_utf8_lossy(&window[..nul]).into_owned()),
            None => Err(unterminated()),
        }
    }

    // ------------------------------------------------------------------ //
    // headers
    // ------------------------------------------------------------------ //

    fn parse_headers(&mut self) -> Result<(), PEError> {
        if self.data.len() < 0x40 {
            return Err(PEError("file is too small to be a PE image".into()));
        }
        if self.u16_at(0)? != DOS_SIGNATURE {
            return Err(PEError("not a PE image: missing 'MZ' signature".into()));
        }

        let e_lfanew = self.u32_at(0x3C)? as usize;
        if e_lfanew == 0 || e_lfanew + 24 > self.data.len() {
            return Err(PEError(format!("bogus e_lfanew (0x{:x})", e_lfanew)));
        }
        if self.u32_at(e_lfanew)? != NT_SIGNATURE {
            return Err(PEError(
                "not a PE image: missing 'PE\\0\\0' signature".into(),
            ));
        }

        let coff = e_lfanew + 4;
        let machine = self.u16_at(coff)?;
        let section_count = self.u16_at(coff + 2)? as usize;
        let timestamp = self.u32_at(coff + 8)?;
        let size_of_optional = self.u16_at(coff + 16)? as usize;

        let optional = coff + 20;
        let magic = self.u16_at(optional)?;
        let (is_64bit, image_base, dir_count_offset, dir_offset) = match magic {
            MAGIC_PE32PLUS => (
                true,
                self.u64_at(optional + 24)?,
                optional + 108,
                optional + 112,
            ),
            MAGIC_PE32 => (
                false,
                self.u32_at(optional + 28)? as u64,
                optional + 92,
                optional + 96,
            ),
            other => {
                return Err(PEError(format!(
                    "unknown optional header magic 0x{:x}",
                    other
                )))
            }
        };

        let directory_count = (self.u32_at(dir_count_offset)? as usize).min(16);
        let mut directories = Vec::with_capacity(directory_count);
        for index in 0..directory_count {
            directories.push((
                self.u32_at(dir_offset + 8 * index)?,
                self.u32_at(dir_offset + 8 * index + 4)?,
            ));
        }

        let section_offset = optional + size_of_optional;
        let mut sections = Vec::with_capacity(section_count);
        for index in 0..section_count {
            let base = section_offset + 40 * index;
            if base + 40 > self.data.len() {
                return Err(PEError(format!(
                    "truncated section table at section {}",
                    index
                )));
            }
            let raw_name = self
                .data
                .get(base..base + 8)
                .ok_or_else(|| PEError(format!("truncated section table at section {}", index)))?;
            let name_bytes: Vec<u8> = raw_name
                .iter()
                .copied()
                .take_while(|&byte| byte != 0)
                .collect();
            sections.push(Section {
                name: String::from_utf8_lossy(&name_bytes).into_owned(),
                virtual_size: self.u32_at(base + 8)?,
                virtual_address: self.u32_at(base + 12)?,
                size_of_raw_data: self.u32_at(base + 16)?,
                pointer_to_raw_data: self.u32_at(base + 20)?,
                characteristics: self.u32_at(base + 36)?,
            });
        }

        self.machine = machine;
        self.timestamp = timestamp;
        self.is_64bit = is_64bit;
        self.image_base = image_base;
        self.directories = directories;
        self.sections = sections;
        Ok(())
    }

    // ------------------------------------------------------------------ //
    // address translation
    // ------------------------------------------------------------------ //

    pub fn directory(&self, index: usize) -> (u32, u32) {
        self.directories.get(index).copied().unwrap_or((0, 0))
    }

    pub fn section_for_rva(&self, rva: u32) -> Option<&Section> {
        self.sections.iter().find(|section| section.contains_rva(rva))
    }

    /// Translate a file offset back to an RVA, if it lives in a section.
    pub fn offset_to_rva(&self, offset: u32) -> Option<u32> {
        for section in &self.sections {
            let start = section.pointer_to_raw_data;
            if start != 0
                && offset >= start
                && offset < start.saturating_add(section.size_of_raw_data)
            {
                return Some(section.virtual_address + (offset - start));
            }
        }
        None
    }

    /// RVAs of every NUL-terminated ASCII occurrence of `text`.
    pub fn find_ascii_string(&self, text: &str) -> Vec<u32> {
        let mut needle = text.as_bytes().to_vec();
        needle.push(0);

        let mut found = Vec::new();
        let mut start = 0usize;
        while let Some(index) = find_subslice(&self.data, &needle, start) {
            if let Some(rva) = self.offset_to_rva(index as u32) {
                found.push(rva);
            }
            start = index + 1;
        }
        found
    }

    pub fn rva_to_offset(&self, rva: u32) -> Result<usize, PEError> {
        let section = self
            .section_for_rva(rva)
            .ok_or_else(|| PEError(format!("RVA 0x{:x} is not inside any section", rva)))?;

        let delta = rva - section.virtual_address;
        if delta >= section.size_of_raw_data {
            return Err(PEError(format!(
                "RVA 0x{:x} falls in the uninitialised tail of section '{}' \
                 and has no bytes on disk",
                rva, section.name
            )));
        }
        let offset = section.pointer_to_raw_data as u64 + delta as u64;
        if offset >= self.data.len() as u64 {
            return Err(PEError(format!(
                "RVA 0x{:x} maps past the end of the file",
                rva
            )));
        }
        Ok(offset as usize)
    }

    // ------------------------------------------------------------------ //
    // exports
    // ------------------------------------------------------------------ //

    /// Every named export, in name-table order.
    pub fn exports(&self) -> Result<Vec<Export>, PEError> {
        let (table_rva, table_size) = self.directory(DIR_EXPORT);
        if table_rva == 0 {
            return Ok(Vec::new());
        }

        let base = self.rva_to_offset(table_rva)?;
        let _characteristics = self.u32_at(base)?;
        let _timestamp = self.u32_at(base + 4)?;
        let _major = self.u16_at(base + 8)?;
        let _minor = self.u16_at(base + 10)?;
        let _name_rva = self.u32_at(base + 12)?;
        let ordinal_base = self.u32_at(base + 16)?;
        let _function_count = self.u32_at(base + 20)?;
        let name_count = self.u32_at(base + 24)? as usize;
        let functions_rva = self.u32_at(base + 28)?;
        let names_rva = self.u32_at(base + 32)?;
        let ordinals_rva = self.u32_at(base + 36)?;

        let functions = if functions_rva != 0 {
            self.rva_to_offset(functions_rva)?
        } else {
            0
        };
        let names = if names_rva != 0 {
            self.rva_to_offset(names_rva)?
        } else {
            0
        };
        let ordinals = if ordinals_rva != 0 {
            self.rva_to_offset(ordinals_rva)?
        } else {
            0
        };
        if functions == 0 || names == 0 || ordinals == 0 {
            return Ok(Vec::new());
        }

        let mut found = Vec::with_capacity(name_count);
        for index in 0..name_count {
            let name = self.cstring(
                self.rva_to_offset(self.u32_at(names + 4 * index)?)?,
                4096,
            )?;
            let ordinal_index = self.u16_at(ordinals + 2 * index)? as usize;
            let function_rva = self.u32_at(functions + 4 * ordinal_index)?;

            // A "function" RVA that points back inside the export directory is
            // not code at all -- it is a forwarder string like "NTDLL.RtlFoo".
            let forwarder = if function_rva >= table_rva
                && (function_rva as u64) < table_rva as u64 + table_size as u64
            {
                Some(self.cstring(self.rva_to_offset(function_rva)?, 4096)?)
            } else {
                None
            };

            found.push(Export {
                name,
                ordinal: ordinal_base.wrapping_add(ordinal_index as u32),
                rva: function_rva,
                forwarder,
            });
        }
        Ok(found)
    }

    /// Exports whose (possibly C++-mangled) name contains `needle`.
    pub fn find_exports(&self, needle: &str) -> Result<Vec<Export>, PEError> {
        Ok(self
            .exports()?
            .into_iter()
            .filter(|export| export.name.contains(needle))
            .collect())
    }

    /// Lowest export RVA strictly greater than `rva`, if any.
    ///
    /// Used only to bound a scan window -- it is an approximation of where a
    /// function ends, not a real control-flow analysis.
    pub fn next_export_rva_after(&self, rva: u32) -> Result<Option<u32>, PEError> {
        Ok(self
            .exports()?
            .into_iter()
            .filter(|export| !export.is_forwarder() && export.rva > rva)
            .map(|export| export.rva)
            .min())
    }

    // ------------------------------------------------------------------ //
    // imports
    // ------------------------------------------------------------------ //

    /// `(dll name, {imported symbol: RVA of its IAT slot})`, in descriptor order.
    pub fn imports(&self) -> Result<Vec<(String, BTreeMap<String, u32>)>, PEError> {
        let (table_rva, _size) = self.directory(DIR_IMPORT);
        if table_rva == 0 {
            return Ok(Vec::new());
        }

        let entry_size = if self.is_64bit { 8usize } else { 4usize };
        let ordinal_flag: u64 = if self.is_64bit {
            1u64 << 63
        } else {
            1u64 << 31
        };

        let mut result = Vec::new();
        let mut descriptor = self.rva_to_offset(table_rva)?;
        loop {
            let original_thunk = self.u32_at(descriptor)?;
            let name_rva = self.u32_at(descriptor + 12)?;
            let first_thunk = self.u32_at(descriptor + 16)?;
            if original_thunk == 0 && name_rva == 0 && first_thunk == 0 {
                break;
            }

            let dll_name = self.cstring(self.rva_to_offset(name_rva)?, 4096)?;
            let mut symbols: BTreeMap<String, u32> = BTreeMap::new();

            let thunk_rva = if original_thunk != 0 {
                original_thunk
            } else {
                first_thunk
            };
            let thunk = self.rva_to_offset(thunk_rva)?;
            let mut index = 0usize;
            loop {
                let value = if self.is_64bit {
                    self.u64_at(thunk + entry_size * index)?
                } else {
                    self.u32_at(thunk + entry_size * index)? as u64
                };
                if value == 0 {
                    break;
                }
                let symbol = if value & ordinal_flag != 0 {
                    format!("#{}", value & 0xFFFF)
                } else {
                    // Skip the 2-byte hint that precedes the name.
                    self.cstring(self.rva_to_offset(value as u32)? + 2, 4096)?
                };
                symbols.insert(symbol, first_thunk + (entry_size * index) as u32);
                index += 1;
            }

            result.push((dll_name, symbols));
            descriptor += 20;
        }

        Ok(result)
    }

    /// Locate an imported symbol by name; returns `(dll, iat_rva)`.
    pub fn find_import(&self, symbol: &str) -> Result<Option<(String, u32)>, PEError> {
        for (dll, symbols) in self.imports()? {
            if let Some(&iat_rva) = symbols.get(symbol) {
                return Ok(Some((dll, iat_rva)));
            }
        }
        Ok(None)
    }

    // ------------------------------------------------------------------ //
    // RIP-relative reference scan
    // ------------------------------------------------------------------ //

    /// Find RIP-relative instructions near `from_rva` that reach `target_rva`.
    ///
    /// A deliberately narrow pattern match over a handful of common x86-64
    /// encodings, not a disassembler. It exists to raise confidence that a
    /// function still reaches a given IAT slot; a negative result is not proof
    /// of absence, because the call may be one level indirect.
    pub fn find_rip_references(&self, from_rva: u32, target_rva: u32, window: usize) -> Result<Vec<RipRef>, PEError> {
        let start = self.rva_to_offset(from_rva)?;
        let end = (start + window).min(self.data.len());
        let blob = self.data.get(start..end).unwrap_or(&[]);

        let mut found = Vec::new();
        for index in 0..blob.len() {
            let Some((length, displacement_at, mnemonic)) = match_rip_form(blob, index) else {
                continue;
            };
            if index + length > blob.len() {
                continue;
            }
            let displacement = i32::from_le_bytes(
                blob[index + displacement_at..index + displacement_at + 4]
                    .try_into()
                    .expect("4-byte window"),
            );
            // Compute in i64: the sum can go negative for a bogus displacement.
            let reaches = from_rva as i64 + index as i64 + length as i64 + displacement as i64;
            if reaches == target_rva as i64 {
                found.push(RipRef {
                    rva: from_rva + index as u32,
                    length,
                    mnemonic,
                });
            }
        }
        Ok(found)
    }

    /// Candidate `call rel32` sites near `from_rva`, as `(call_site_rva, target_rva)`.
    ///
    /// `0xE8` also occurs inside other instructions' operands, so these are
    /// candidates only -- callers must validate the target before trusting one.
    /// Targets are returned as `i64` because a bogus displacement can point
    /// before the image.
    pub fn find_relative_calls(&self, from_rva: u32, window: usize) -> Result<Vec<(i64, i64)>, PEError> {
        let start = self.rva_to_offset(from_rva)?;
        let end = (start + window).min(self.data.len());
        let blob = self.data.get(start..end).unwrap_or(&[]);
        if blob.len() < 5 {
            return Ok(Vec::new());
        }

        let mut found = Vec::new();
        for index in 0..=(blob.len() - 5) {
            if blob[index] != 0xE8 {
                continue;
            }
            let displacement = i32::from_le_bytes(
                blob[index + 1..index + 5].try_into().expect("4-byte window"),
            );
            let site = from_rva as i64 + index as i64;
            found.push((site, site + 5 + displacement as i64));
        }
        Ok(found)
    }

    /// Is the code at `rva` a bare `jmp qword [rip+disp32]` to `iat_rva`?
    ///
    /// Compilers routinely route imported calls through a one-instruction
    /// thunk, so a function that calls an import often contains no direct
    /// reference to the IAT slot at all.
    pub fn is_import_thunk_for(&self, rva: i64, iat_rva: u32) -> bool {
        if rva < 0 {
            return false;
        }
        let Ok(offset) = self.rva_to_offset(rva as u32) else {
            return false;
        };

        let end = (offset + 16).min(self.data.len());
        let Some(blob) = self.data.get(offset..end) else {
            return false;
        };

        let mut cursor = 0usize;
        // Skip a CET landing pad if one is present.
        if blob.len() >= 4 && blob[..4] == [0xf3, 0x0f, 0x1e, 0xfa] {
            cursor = 4;
        }
        if blob.len() < cursor + 6 {
            return false;
        }
        if blob[cursor] != 0xFF || blob[cursor + 1] != 0x25 {
            return false;
        }
        let displacement = i32::from_le_bytes(
            blob[cursor + 2..cursor + 6].try_into().expect("4-byte window"),
        );
        rva + cursor as i64 + 6 + displacement as i64 == iat_rva as i64
    }

    // ------------------------------------------------------------------ //
    // misc
    // ------------------------------------------------------------------ //

    pub fn read_at_rva(&self, rva: u32, length: usize) -> Result<Vec<u8>, PEError> {
        let offset = self.rva_to_offset(rva)?;
        let end = (offset + length).min(self.data.len());
        Ok(self.data.get(offset..end).unwrap_or(&[]).to_vec())
    }

    pub fn machine_name(&self) -> &'static str {
        match self.machine {
            MACHINE_AMD64 => "x86-64",
            MACHINE_I386 => "x86",
            _ => "unknown",
        }
    }
}

/// Recognise a few RIP-relative encodings at `index`.
///
/// Returns `(instruction_length, displacement_offset, mnemonic)` or `None`.
fn match_rip_form(blob: &[u8], index: usize) -> Option<(usize, usize, &'static str)> {
    let remaining = blob.len() - index;

    if remaining >= 6 && blob[index] == 0xFF {
        if blob[index + 1] == 0x15 {
            return Some((6, 2, "call [rip+disp32]"));
        }
        if blob[index + 1] == 0x25 {
            return Some((6, 2, "jmp [rip+disp32]"));
        }
    }
    if remaining >= 7 && (0x48..=0x4F).contains(&blob[index]) {
        let opcode = blob[index + 1];
        // modrm with mod=00 and rm=101 is the RIP-relative form
        if (opcode == 0x8B || opcode == 0x8D) && (blob[index + 2] & 0xC7) == 0x05 {
            let mnemonic = if opcode == 0x8B {
                "mov r64,[rip+disp32]"
            } else {
                "lea r64,[rip+disp32]"
            };
            return Some((7, 3, mnemonic));
        }
    }
    None
}

/// `haystack.find(needle, start)`, without pulling in a substring crate.
fn find_subslice(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() || start > haystack.len() - needle.len()
    {
        return None;
    }
    (start..=haystack.len() - needle.len()).find(|&index| &haystack[index..index + needle.len()] == needle)
}
