# Flux Rec Launcher (v2)

Tauri 2 launcher: Rust backend + vanilla JS frontend.

## Layout

- `core/` — `fluxrec-core` library: all business logic (updates, BepInEx, bypass, launch).
  Compiles and tests on any platform.
- `src-tauri/` — Tauri 2 app shell, exposes core as Tauri commands.
- `src/` — frontend (index.html + app.js + styles.css).

## The three update channels (anti-wipe rule)

| Channel | Source | May only write |
|---|---|---|
| A — launcher | GitHub `recflare-installer-v*` releases | the launcher exe itself |
| B — game files | `manifest.json` on the HF mirror | files listed in the manifest |
| C — plugin | `RecNetPlugin.dll.sha256` sidecar | `BepInEx/plugins/RecNetPlugin.dll` |

A channel update never touches another channel's files. Version tokens are
written last, after verification. Every failure falls back to "play anyway".

## Build

```sh
cargo build            # core + tauri app (Linux needs webkit2gtk dev libs)
cargo test -p fluxrec-core
```

Windows release build happens in CI (`tauri-action`) on `windows-latest`.

## Tauri commands

`get_settings`, `save_settings`, `get_status`, `game_present`,
`check_updates`, `apply_updates` (B+C), `fetch_launcher_update` (A),
`play`, `verify_files`, `repair_files`, `repair_bepinex`, `reset_plugin`,
`get_changelog`, `open_game_folder`, `copy_diagnostics`, `defender_steps`.

Progress: `launcher://progress` events with `{channel, message, file_done, file_total, overall}`.
