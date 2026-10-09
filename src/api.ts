import { invoke } from '@tauri-apps/api/core'
import type {
  CleanOutcome,
  EnvReport,
  InstallDto,
  LogReport,
  PatchOutcome,
  ReportDto,
  RestoreOutcome,
} from './types'

// Tauri converts these camelCase argument names to the snake_case parameters
// the Rust commands declare.

export function listInstalls(): Promise<InstallDto[]> {
  return invoke<InstallDto[]>('list_installs')
}

export function resolveInstall(path: string): Promise<InstallDto> {
  return invoke<InstallDto>('resolve_install', { path })
}

/** Read-only. Passing no path falls back to the remembered install. */
export function status(installPath?: string | null): Promise<ReportDto> {
  return invoke<ReportDto>('status', { installPath: installPath ?? null })
}

/**
 * `expectedSha256` is the hash the UI displayed. The backend re-inspects and
 * refuses if the DLL changed underneath, which is what stops a stale dialog
 * from writing over a freshly-updated game.
 */
export function patch(installPath: string | null, expectedSha256: string): Promise<PatchOutcome> {
  return invoke<PatchOutcome>('patch', { installPath, expectedSha256 })
}

export function restore(installPath?: string | null): Promise<RestoreOutcome> {
  return invoke<RestoreOutcome>('restore', { installPath: installPath ?? null })
}

export function clean(installPath?: string | null): Promise<CleanOutcome> {
  return invoke<CleanOutcome>('clean', { installPath: installPath ?? null })
}

/** `includeWine` starts a wineserver, so it is opt-in and never automatic. */
export function checkEnv(includeWine: boolean, bottle: string): Promise<EnvReport> {
  return invoke<EnvReport>('check_env', { includeWine, bottle })
}

export function scanLog(installPath?: string | null): Promise<LogReport> {
  return invoke<LogReport>('scan_log', { installPath: installPath ?? null })
}

export function runningProcesses(): Promise<string[]> {
  return invoke<string[]>('running_processes')
}

export function statePath(): Promise<string> {
  return invoke<string>('state_path')
}

export function revealInFinder(path: string): Promise<void> {
  return invoke<void>('reveal_in_finder', { path })
}

export function openTerminal(command: string): Promise<void> {
  return invoke<void>('open_terminal', { command })
}

/** Native folder picker. Resolves to null when the user cancels. */
export function pickFolder(prompt: string): Promise<string | null> {
  return invoke<string | null>('pick_folder', { prompt })
}
