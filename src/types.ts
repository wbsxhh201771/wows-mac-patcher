// Mirrors the Rust DTOs in src-tauri/src/patcher/dto.rs and diag.rs.
// Field names are snake_case because that is what serde emits.

export type PatchState = 'ORIGINAL' | 'PATCHED' | 'FOREIGN' | 'UNRESOLVED'

export interface InstallDto {
  root: string
  build: string | null
  version: string | null
  build_source: string
  available_builds: string[]
  patchable_builds: string[]
  incomplete_builds: string[]
}

export interface SiteDto {
  export_name: string
  export_ordinal: number
  export_rva: number
  file_offset: number
  section: string
  section_is_code: boolean
  current_bytes: string
  wine_import_dll: string | null
  wine_iat_rva: number | null
  wine_string_rva: number | null
  wine_reference: string | null
  is_patched: boolean
}

export interface DllInfoDto {
  path: string
  name: string
  exists: boolean
  size: number
  sha256: string
  error: string | null
  site: SiteDto | null
}

export interface SidecarsDto {
  count: number
  in_live_build: number
  by_folder: [string, number][]
  cancelled: boolean
}

export interface ReportDto {
  state: PatchState
  state_help: string
  error: string | null
  notes: string[]
  can_patch: boolean
  can_restore: boolean
  needs_clean: boolean
  install: InstallDto | null
  target: DllInfoDto | null
  backup: DllInfoDto | null
  record: unknown
  sidecars: SidecarsDto | null
  patch_bytes: string
  patch_disasm: string
  export_needle: string
  wine_import: string
}

export interface CleanOutcome {
  found: number
  removed: number
  failures: string[]
}

export interface RestoreOutcome {
  dll: string
  backup: string
  sha256_restored: string
  restored_at: string
}

export interface PatchOutcome {
  file_offset: number
  original_bytes: string
  patched_bytes: string
  backup: string
  sha256_original: string
  sha256_patched: string
}

export interface LogSession {
  started: string
  errors: number
  local_address_failures: number
  verdict: string
  healthy: boolean
}

export interface LogReport {
  path: string
  exists: boolean
  size: number
  modified: string | null
  sessions: LogSession[]
}

export interface EnvReport {
  host_name: string
  local_host_name: string
  unix_hostname: string
  resolved_address: string | null
  wine_registry: string | null
  egress_interface: string | null
  hostname_ok: boolean
  vpn_warning: boolean
  fix_command: string | null
  dead_ends: string[]
  notes: string[]
}

export interface ScanProgress {
  scanned: number
  found: number
  current: string
}
