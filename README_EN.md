# WOW-Crossover-patcher

[中文](./README.md) · English

World of Warships runs fine under CrossOver on macOS. It just refuses to.

Before any graphics or DirectX code runs, the client asks Wine what the host
operating system is, finds `Darwin`, prints **"Sorry, unsupported operating
system: macOS support has been discontinued"**, and exits. Nothing actually
failed — the client simply declined to continue.

This tool flips that one check, and can flip it back.

```
WorldOfWarships64.exe
  -> platform64.dll!getHostVersionFromWine()
     -> ntdll!wine_get_host_version()
        -> lowercase the host system name
           -> contains "darwin"?  ->  refuse to start
```

Making `getHostVersionFromWine()` return `0` removes the refusal. That is a
three-byte change:

```
31 C0        xor eax, eax
C3           ret
```

## What this is

**WoWs Mac Patcher** — a macOS GUI app (Rust + Tauri 2 + React).

| | |
| --- | --- |
| Code | `src-tauri/` + `src/` |
| Develop | `npm install && npm run tauri dev` |
| Release | `npm run release:mac` (artifacts in `out/`) |
| Requires | macOS 13+, CrossOver, a World of Warships install |

Features: status, patch, restore, `._` sidecar cleanup, environment and log
diagnostics.

## Where this came from

The refusal is a *string*, so that string has to be sitting in a binary
somewhere, and whatever code references it is the check worth reading. Following
that leads to the call path above.

What pushed the work over the line was
[this CodeWeavers forum thread](https://www.codeweavers.com/compatibility/crossover/forum/world-of-warships?msg=358452),
which published the same analysis and showed someone had patched it and the game
ran. Credit to them for posting it first.

The deliberate difference from that post is the **offset**. The post pins a
single file offset, `0x570`, and warns not to reuse it after an update. This
tool never hardcodes an offset: every run re-derives the patch site from the
DLL's export table. That mattered immediately — on a Steam install of the *same*
build number, the right offset is `0x5B0`, and `0x570` lands mid-instruction in
an unrelated function.

## Use

1. Build with `npm run release:mac`, or open an existing `.app` from `out/`
2. Launch the app and confirm the install path and status (read-only by default)
3. Confirm, then patch; use restore when you need to undo

`status` is always read-only. `patch` / `clean` always require confirmation.
Root-requiring hostname fixes are **never** auto-elevated — the app only
suggests the command and can open Terminal for you.

### States

| State | Meaning | `patch` | `restore` |
| --- | --- | --- | --- |
| `ORIGINAL` | Not patched | yes | — |
| `PATCHED` | Patched by this tool, verified backup present | — | yes |
| `FOREIGN` | Modified, but the backup is missing, stale, or itself patched | no | no |
| `UNRESOLVED` | Could not find the function to patch | no | no |

`FOREIGN` is a dead end. Get a clean DLL from your launcher — Steam's **Verify
integrity of game files** or Wargaming Game Center's **Check and Repair** — and
start over.

### Where state lives

- `platform64_bck.dll` — the backup, beside the DLL it came from
- `~/Library/Application Support/WoWsMacPatcher/state.json` — build number,
  offsets, original and patched SHA-256 hashes, timestamps

## How it stays safe

**Offsets are resolved, never remembered.** Every run parses the PE export
table, finds the export whose name contains `getHostVersionFromWine`, and
converts its RVA to a file offset through the section table. When Wargaming
ships an update, the offset moves and the tool follows it.

**It stops rather than guesses.** Missing export, ambiguous match, forwarder, or
resolution outside an executable section → `UNRESOLVED` / `FOREIGN`, no write.

**Backups are made from originals only.** A backup is only created when the
patch site does *not* already hold `31 C0 C3`. That prevents the classic
disaster: patch, update, patch again, backup silently overwritten with an
already-patched file.

**Writes are atomic.** Changes go to a temporary file in the same directory,
then replace the original.

## After a game update

The patch does not survive updates, and it should not. Launchers replace
`platform64.dll` — that is the safe outcome. Open the app and patch again; the
tool re-derives everything for the new build.

`patch` also removes macOS `._` metadata files left by the update (see below).

## Troubleshooting

### "No resource paths are loaded from command line or paths.xml"

The game crashes right after launch, usually after the first update run on the
Mac. The patch is fine; the update left thousands of hidden `._` files.

![Crash dialog](docs/no-resource-paths-crash.png)

Use **Clean** in the app. External drives are usually exFAT and cannot store
extended attributes, so macOS writes AppleDouble sidecars; Wine treats them as
ordinary files, and the client fails parsing `._basecontent.idx`.

Cleanup only removes `._*` files that begin with the AppleDouble header
(`00 05 16 07`). Keeping the game on APFS avoids them entirely — but Windows
cannot read APFS.

## Risks

Read this part.

- This modifies a signed game file. The DLL's digital signature will no longer
  validate.
- It may conflict with Wargaming's terms of service. Only Wargaming can tell you
  what that means for your account.
- A future integrity or anti-cheat check could reject a modified DLL. If
  anything objects, restore immediately.
- Your launcher may replace the DLL and undo the patch — that is the safe
  outcome. Just patch again.
- Nothing here touches World of Tanks, `WorldOfWarships64.exe`, Wine, CrossOver,
  or the registry. The only files it ever deletes are macOS `._` sidecars, and
  only after confirmation.

Use at your own risk.

## Development

```bash
npm install
npm run tauri dev

# Tests (PE fixture synthesised in Rust; no mingw needed)
cargo test --manifest-path src-tauri/Cargo.toml --lib

# Release
npm run release:mac
```

```
src-tauri/src/patcher/
  pe.rs           PE reader: exports, imports, sections, RVA math
  locate.rs       install discovery and live-build resolution
  patch.rs        state machine, backup rules, atomic writes
  appledouble.rs  finds and removes macOS '._' sidecars
  manifest.rs     durable record of what was changed
  diag.rs         environment and log diagnostics
  fixture.rs      synthetic PE64 DLL for tests (#[cfg(test)] only)
src/              React frontend
```

## Verified

Working on **macOS 26.5.1 (25F80)** with **CrossOver 26.2**, World of Warships
build **13015811** from **Steam**, patched at file offset `0x5B0`. This is the
Steam `platform64.dll` (version resource: `"Steam Intergation Module"`); the
Wargaming Game Center binary for the same version number is different, which is
why the forum post's `0x570` does not apply here.
