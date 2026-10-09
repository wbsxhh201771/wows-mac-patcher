//! Synthetic PE64 DLL used by the patcher test suite.
//!
//! Mimics the shape of `platform64.dll` that the engine cares about:
//! mangled exports (including decoys), an `ntdll!wine_get_host_version`
//! import reached through a one-instruction thunk, and a
//! `GetProcAddress`-style function that `lea`s the symbol-name string.

const FILE_ALIGN: u32 = 0x200;
const SECT_ALIGN: u32 = 0x1000;

const TEXT_RVA: u32 = 0x1000;
const RDATA_RVA: u32 = 0x2000;

// .text layout (offsets from TEXT_RVA)
const FN_HOST_VERSION: u32 = 0x00;
const FN_VIA_PROC: u32 = 0x20;
const FN_COMPUTE: u32 = 0x40;
const FN_HOST_NAME: u32 = 0x50;
const THUNK: u32 = 0x60;
const TEXT_SIZE: u32 = 0x80;

fn align(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) & !(alignment - 1)
}

fn put_u16(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(buf: &mut [u8], offset: usize, value: u64) {
    buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Build a minimal PE64 DLL that exercises the PE / patch regression suite.
pub fn platform64_dll() -> Vec<u8> {
    // --- .rdata contents (built first so RVAs are known) -----------------
    // Layout inside .rdata:
    //   0x00  export directory (40)
    //   0x28  EAT (4 * 4)
    //   0x38  name pointers (4 * 4)
    //   0x48  ordinals (4 * 2)
    //   0x50  export DLL name
    //   0x60  export names (packed, null-terminated)
    //   0xF0  import descriptor (20) + null (20)
    //   0x118 ILT (8 + 8)
    //   0x128 IAT (8 + 8)
    //   0x138 hint/name for wine_get_host_version
    //   0x158 "ntdll.dll"
    let mut rdata = vec![0u8; 0x180];

    let export_dir_off = 0x00usize;
    let eat_off = 0x28usize;
    let names_ptr_off = 0x38usize;
    let ords_off = 0x48usize;
    let export_dll_off = 0x50usize;
    let export_names_off = 0x60usize;

    let import_desc_off = 0xF0usize;
    let ilt_off = 0x118usize;
    let iat_off = 0x128usize;
    let hint_name_off = 0x138usize;
    let ntdll_name_off = 0x158usize;

    let export_names: [&[u8]; 4] = [
        b"?computeSomethingElse@@YAHH@Z\0",
        b"?getHostName@@YAHPEADH@Z\0",
        b"?getHostVersionFromWine@@YA_KXZ\0",
        b"?getHostVersionViaProcAddress@@YA_KXZ\0",
    ];
    // Alphabetical order matches AddressOfNames; ordinals index into EAT.
    let eat_rvas: [u32; 4] = [
        TEXT_RVA + FN_COMPUTE,
        TEXT_RVA + FN_HOST_NAME,
        TEXT_RVA + FN_HOST_VERSION,
        TEXT_RVA + FN_VIA_PROC,
    ];

    rdata[export_dll_off..export_dll_off + 15].copy_from_slice(b"platform64.dll\0");

    let mut name_cursor = export_names_off;
    let mut name_rvas = [0u32; 4];
    for (index, name) in export_names.iter().enumerate() {
        name_rvas[index] = RDATA_RVA + name_cursor as u32;
        rdata[name_cursor..name_cursor + name.len()].copy_from_slice(name);
        name_cursor += name.len();
    }

    // Export directory
    put_u32(&mut rdata, export_dir_off + 12, RDATA_RVA + export_dll_off as u32); // Name
    put_u32(&mut rdata, export_dir_off + 16, 1); // Base
    put_u32(&mut rdata, export_dir_off + 20, 4); // NumberOfFunctions
    put_u32(&mut rdata, export_dir_off + 24, 4); // NumberOfNames
    put_u32(&mut rdata, export_dir_off + 28, RDATA_RVA + eat_off as u32);
    put_u32(&mut rdata, export_dir_off + 32, RDATA_RVA + names_ptr_off as u32);
    put_u32(&mut rdata, export_dir_off + 36, RDATA_RVA + ords_off as u32);

    for (index, &rva) in eat_rvas.iter().enumerate() {
        put_u32(&mut rdata, eat_off + index * 4, rva);
        put_u32(&mut rdata, names_ptr_off + index * 4, name_rvas[index]);
        put_u16(&mut rdata, ords_off + index * 2, index as u16);
    }

    // Hint/Name: hint 0 + "wine_get_host_version\0"
    let wine_name = b"wine_get_host_version\0";
    rdata[hint_name_off + 2..hint_name_off + 2 + wine_name.len()].copy_from_slice(wine_name);
    let wine_string_rva = RDATA_RVA + hint_name_off as u32 + 2;

    rdata[ntdll_name_off..ntdll_name_off + 10].copy_from_slice(b"ntdll.dll\0");

    let hint_name_rva = RDATA_RVA + hint_name_off as u32;
    put_u64(&mut rdata, ilt_off, hint_name_rva as u64);
    put_u64(&mut rdata, iat_off, hint_name_rva as u64); // unbound IAT mirrors ILT

    // Import descriptor for ntdll.dll
    put_u32(&mut rdata, import_desc_off, RDATA_RVA + ilt_off as u32); // OriginalFirstThunk
    put_u32(&mut rdata, import_desc_off + 12, RDATA_RVA + ntdll_name_off as u32);
    put_u32(&mut rdata, import_desc_off + 16, RDATA_RVA + iat_off as u32); // FirstThunk
    // trailing null descriptor already zeroed

    let rdata_raw_size = align(rdata.len() as u32, FILE_ALIGN);
    rdata.resize(rdata_raw_size as usize, 0);

    let iat_rva = RDATA_RVA + iat_off as u32;

    // --- .text -----------------------------------------------------------
    let mut text = vec![0u8; TEXT_SIZE as usize];

    // getHostVersionFromWine: prologue + call thunk + xor eax,eax ; ret
    let host_off = FN_HOST_VERSION as usize;
    text[host_off..host_off + 5].copy_from_slice(&[0x48, 0x89, 0x5C, 0x24, 0x08]);
    let call_at = host_off + 5;
    text[call_at] = 0xE8;
    let call_rva = TEXT_RVA + call_at as u32;
    let thunk_rva = TEXT_RVA + THUNK;
    let call_disp = thunk_rva as i32 - (call_rva as i32 + 5);
    put_u32(&mut text, call_at + 1, call_disp as u32);
    text[call_at + 5] = 0x31;
    text[call_at + 6] = 0xC0;
    text[call_at + 7] = 0xC3;

    // getHostVersionViaProcAddress: lea rax, [rip+disp] ; xor eax,eax ; ret
    let via_off = FN_VIA_PROC as usize;
    text[via_off] = 0x48;
    text[via_off + 1] = 0x8D;
    text[via_off + 2] = 0x05; // ModRM: rax, [rip+disp32]
    let lea_rva = TEXT_RVA + via_off as u32;
    let lea_disp = wine_string_rva as i32 - (lea_rva as i32 + 7);
    put_u32(&mut text, via_off + 3, lea_disp as u32);
    text[via_off + 7] = 0x31;
    text[via_off + 8] = 0xC0;
    text[via_off + 9] = 0xC3;

    // computeSomethingElse: lea eax, [rcx+1] ; ret
    let compute_off = FN_COMPUTE as usize;
    text[compute_off..compute_off + 4].copy_from_slice(&[0x8D, 0x41, 0x01, 0xC3]);

    // getHostName: xor eax,eax ; ret
    let name_off = FN_HOST_NAME as usize;
    text[name_off..name_off + 3].copy_from_slice(&[0x31, 0xC0, 0xC3]);

    // Import thunk: jmp qword [rip+disp32] -> IAT
    let thunk_off = THUNK as usize;
    text[thunk_off] = 0xFF;
    text[thunk_off + 1] = 0x25;
    let thunk_disp = iat_rva as i32 - (thunk_rva as i32 + 6);
    put_u32(&mut text, thunk_off + 2, thunk_disp as u32);

    let text_raw_size = align(TEXT_SIZE, FILE_ALIGN);
    text.resize(text_raw_size as usize, 0);

    // --- headers ---------------------------------------------------------
    let e_lfanew = 0x80u32;
    let coff = e_lfanew as usize + 4;
    let optional = coff + 20;
    let size_of_optional = 0xF0u16; // PE32+ with 16 data directories
    let section_headers = optional + size_of_optional as usize;
    let headers_end = section_headers + 2 * 40; // two sections
    let size_of_headers = align(headers_end as u32, FILE_ALIGN);

    let text_raw = size_of_headers;
    let rdata_raw = text_raw + text_raw_size;
    let file_size = rdata_raw + rdata_raw_size;

    let mut file = vec![0u8; file_size as usize];

    // DOS header
    put_u16(&mut file, 0, 0x5A4D); // MZ
    put_u32(&mut file, 0x3C, e_lfanew);

    // PE signature
    put_u32(&mut file, e_lfanew as usize, 0x0000_4550);

    // COFF
    put_u16(&mut file, coff, 0x8664); // AMD64
    put_u16(&mut file, coff + 2, 2); // NumberOfSections
    put_u16(&mut file, coff + 16, size_of_optional);
    put_u16(&mut file, coff + 18, 0x2022); // EXECUTABLE | LARGE_ADDRESS_AWARE | DLL

    // Optional header (PE32+)
    put_u16(&mut file, optional, 0x020B); // PE32+
    file[optional + 2] = 1; // MajorLinkerVersion
    put_u32(&mut file, optional + 16, TEXT_RVA); // AddressOfEntryPoint
    put_u64(&mut file, optional + 24, 0x1800_0000_0000); // ImageBase
    put_u32(&mut file, optional + 32, SECT_ALIGN); // SectionAlignment
    put_u32(&mut file, optional + 36, FILE_ALIGN); // FileAlignment
    put_u16(&mut file, optional + 40, 6); // MajorOperatingSystemVersion
    put_u16(&mut file, optional + 48, 6); // MajorSubsystemVersion
    put_u32(
        &mut file,
        optional + 56,
        align(RDATA_RVA + rdata_raw_size, SECT_ALIGN),
    ); // SizeOfImage
    put_u32(&mut file, optional + 60, size_of_headers); // SizeOfHeaders
    put_u16(&mut file, optional + 68, 3); // Subsystem = IMAGE_SUBSYSTEM_WINDOWS_CUI
    put_u16(&mut file, optional + 70, 0x0160); // DLL characteristics
    put_u64(&mut file, optional + 72, 0x100000); // SizeOfStackReserve
    put_u64(&mut file, optional + 80, 0x1000); // SizeOfStackCommit
    put_u64(&mut file, optional + 88, 0x100000); // SizeOfHeapReserve
    put_u64(&mut file, optional + 96, 0x1000); // SizeOfHeapCommit
    put_u32(&mut file, optional + 108, 16); // NumberOfRvaAndSizes

    // Data directories: Export, Import
    let dirs = optional + 112;
    put_u32(&mut file, dirs, RDATA_RVA + export_dir_off as u32);
    put_u32(&mut file, dirs + 4, 40);
    put_u32(&mut file, dirs + 8, RDATA_RVA + import_desc_off as u32);
    put_u32(&mut file, dirs + 12, 40);

    // Section: .text
    let text_hdr = section_headers;
    file[text_hdr..text_hdr + 5].copy_from_slice(b".text");
    put_u32(&mut file, text_hdr + 8, TEXT_SIZE); // VirtualSize
    put_u32(&mut file, text_hdr + 12, TEXT_RVA);
    put_u32(&mut file, text_hdr + 16, text_raw_size);
    put_u32(&mut file, text_hdr + 20, text_raw);
    put_u32(&mut file, text_hdr + 36, 0x6000_0020); // CODE | EXECUTE | READ

    // Section: .rdata
    let rdata_hdr = section_headers + 40;
    file[rdata_hdr..rdata_hdr + 6].copy_from_slice(b".rdata");
    put_u32(&mut file, rdata_hdr + 8, rdata.len() as u32);
    put_u32(&mut file, rdata_hdr + 12, RDATA_RVA);
    put_u32(&mut file, rdata_hdr + 16, rdata_raw_size);
    put_u32(&mut file, rdata_hdr + 20, rdata_raw);
    put_u32(&mut file, rdata_hdr + 36, 0x4000_0040); // INITIALIZED_DATA | READ

    file[text_raw as usize..text_raw as usize + text.len()].copy_from_slice(&text);
    file[rdata_raw as usize..rdata_raw as usize + rdata.len()].copy_from_slice(&rdata);

    file
}
