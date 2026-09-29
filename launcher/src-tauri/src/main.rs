//! Flux Rec Launcher — Tauri 2 entry point.
//!
//! Modes:
//! - default: run the launcher UI.
//! - `--apply-update <new-exe>`: helper that swaps the launcher binary after
//!   the main process exits, then relaunches (Channel A final step).

#![cfg_attr(windows, windows_subsystem = "windows")]

use fluxrec_core::download::ProgressCb;
use fluxrec_core::{HealthStatus, ProgressEvent, UpdateCheck};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;

fn emit_progress(app: &tauri::AppHandle, ev: ProgressEvent) {
    let _ = app.emit("launcher://progress", ev);
}

fn progress_cb(app: &tauri::AppHandle) -> ProgressCb {
    let app = app.clone();
    Box::new(move |ev: ProgressEvent| emit_progress(&app, ev))
}

fn game_dir() -> PathBuf {
    fluxrec_core::settings::LauncherSettings::load().game_dir()
}

// ---------- settings ----------

#[tauri::command]
fn get_settings() -> fluxrec_core::settings::LauncherSettings {
    fluxrec_core::settings::LauncherSettings::load()
}

#[tauri::command]
fn save_settings(s: fluxrec_core::settings::LauncherSettings) -> Result<(), String> {
    s.save().map_err(|e| e.to_string())
}

// ---------- status / health ----------

#[tauri::command]
async fn get_status(app: tauri::AppHandle) -> Result<HealthStatus, String> {
    let _ = app;
    Ok(fluxrec_core::launch::health(&game_dir()).await)
}

#[tauri::command]
fn game_present() -> bool {
    fluxrec_core::launch::game_present(&game_dir())
}

// ---------- updates: check all three channels ----------

#[tauri::command]
async fn check_updates() -> Result<Vec<UpdateCheck>, String> {
    let settings = fluxrec_core::settings::LauncherSettings::load();
    let beta = settings.channel == "beta";
    let gd = game_dir();
    let (a, b, c) = tokio::join!(
        fluxrec_core::update_launcher::check(beta),
        fluxrec_core::update_content::check(&gd),
        fluxrec_core::plugin_sync::check(&gd),
    );
    // Fail-soft per channel: a failed check becomes "unavailable", never an error.
    let mut out = Vec::new();
    out.push(a.unwrap_or(UpdateCheck {
        channel: "launcher".into(),
        available: false,
        current: env!("CARGO_PKG_VERSION").into(),
        latest: "?".into(),
        detail: "check failed (offline?)".into(),
    }));
    out.push(b.unwrap_or(UpdateCheck {
        channel: "content".into(),
        available: false,
        current: "?".into(),
        latest: "?".into(),
        detail: "check failed (offline?)".into(),
    }));
    out.push(c.unwrap_or(UpdateCheck {
        channel: "plugin".into(),
        available: false,
        current: "?".into(),
        latest: "?".into(),
        detail: "check failed (offline?)".into(),
    }));
    Ok(out)
}

/// Apply content + plugin updates (Channel B + C). Launcher (A) is separate.
#[tauri::command]
async fn apply_updates(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let gd = game_dir();
    let cb = progress_cb(&app);
    let cb = Arc::new(cb);
    let mut log = Vec::new();

    // Channel B
    match fluxrec_core::update_content::fetch_manifest().await {
        Ok(manifest) => {
            let cb2: ProgressCb = {
                let cb = Arc::clone(&cb);
                Box::new(move |e| cb(e))
            };
            match fluxrec_core::update_content::sync(&gd, &manifest, Some(&cb2)).await {
                Ok(rep) => {
                    if rep.failed.is_empty() {
                        log.push(format!(
                            "game files: {} downloaded, {} already ok",
                            rep.downloaded, rep.verified_ok
                        ));
                    } else {
                        log.push(format!(
                            "game files: {} failed: {}",
                            rep.failed.len(),
                            rep.failed.join("; ")
                        ));
                    }
                }
                Err(e) => log.push(format!("game update failed: {e}")),
            }
        }
        Err(e) => log.push(format!("manifest fetch failed (offline?): {e}")),
    }

    // Channel C
    let cb3: ProgressCb = {
        let cb = Arc::clone(&cb);
        Box::new(move |e| cb(e))
    };
    match fluxrec_core::plugin_sync::sync(&gd, Some(&cb3)).await {
        Ok(true) => log.push("plugin up to date".into()),
        Ok(false) => log.push("plugin sync skipped (offline?)".into()),
        Err(e) => log.push(format!("plugin sync failed: {e}")),
    }
    Ok(log)
}

/// Channel A step 1: download + verify the new launcher exe. Returns its path.
/// The frontend then spawns `<new-exe> --apply-update` and exits.
#[tauri::command]
async fn fetch_launcher_update(
    app: tauri::AppHandle,
    latest: String,
    current: String,
) -> Result<String, String> {
    let cb = progress_cb(&app);
    let check = UpdateCheck {
        channel: "launcher".into(),
        available: true,
        current,
        latest,
        detail: String::new(),
    };
    let path = fluxrec_core::update_launcher::fetch_update(&check, Some(&cb))
        .await
        .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

// ---------- play ----------

#[tauri::command]
async fn play(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let gd = game_dir();
    let cb = progress_cb(&app);
    let actions = fluxrec_core::launch::self_heal(&gd, Some(&cb))
        .await
        .map_err(|e| e.to_string())?;
    let settings = fluxrec_core::settings::LauncherSettings::load();
    let pid = fluxrec_core::launch::spawn_game(&gd, settings.no_console).map_err(|e| e.to_string())?;
    let mut out = actions;
    out.push(format!("game launched (pid {pid})"));
    if settings.close_on_play {
        // Give the game a moment, then exit the launcher.
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            std::process::exit(0);
        });
    }
    Ok(out)
}

// ---------- verify / repair ----------

#[tauri::command]
async fn verify_files(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let gd = game_dir();
    let cb = progress_cb(&app);
    let manifest = fluxrec_core::update_content::fetch_manifest()
        .await
        .map_err(|e| e.to_string())?;
    let total = manifest.files.len();
    let mut bad = Vec::new();
    for (i, f) in manifest.files.iter().enumerate() {
        let local = gd.join(&f.path);
        let ok = local.is_file()
            && fluxrec_core::sha256_file(&local)
                .map(|h| h.eq_ignore_ascii_case(&f.sha256))
                .unwrap_or(false);
        if !ok {
            bad.push(f.path.clone());
        }
        if i % 50 == 0 {
            emit_progress(
                &app,
                ProgressEvent {
                    channel: "verify".into(),
                    message: format!("verifying {}/{}", i + 1, total),
                    file_done: i as u64,
                    file_total: total as u64,
                    overall: i as f32 / total.max(1) as f32,
                },
            );
        }
        let _ = &cb;
    }
    Ok(bad)
}

#[tauri::command]
async fn repair_files(app: tauri::AppHandle, paths: Vec<String>) -> Result<Vec<String>, String> {
    // Repair = run the content sync; it only downloads mismatches.
    let gd = game_dir();
    let cb = progress_cb(&app);
    let manifest = fluxrec_core::update_content::fetch_manifest()
        .await
        .map_err(|e| e.to_string())?;
    let filtered = fluxrec_core::update_content::ContentManifest {
        content_version: manifest.content_version,
        files: manifest
            .files
            .into_iter()
            .filter(|f| paths.contains(&f.path))
            .collect(),
    };
    let rep = fluxrec_core::update_content::sync(&gd, &filtered, Some(&cb))
        .await
        .map_err(|e| e.to_string())?;
    let mut out = vec![format!("repaired {}", rep.downloaded)];
    out.extend(rep.failed);
    Ok(out)
}

#[tauri::command]
async fn repair_bepinex(app: tauri::AppHandle) -> Result<String, String> {
    let gd = game_dir();
    let cb = progress_cb(&app);
    fluxrec_core::bepinex::install(&gd, Some(&cb))
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "BepInEx {} reinstalled",
        fluxrec_core::constants::BEPINEX_VERSION
    ))
}

#[tauri::command]
async fn reset_plugin(app: tauri::AppHandle) -> Result<String, String> {
    let gd = game_dir();
    let p = gd.join(fluxrec_core::constants::PLUGIN_REL_PATH);
    let _ = std::fs::remove_file(&p);
    let cb = progress_cb(&app);
    let ok = fluxrec_core::plugin_sync::sync(&gd, Some(&cb))
        .await
        .map_err(|e| e.to_string())?;
    Ok(if ok {
        "plugin reset to mirror version".into()
    } else {
        "plugin reset failed (offline?)".into()
    })
}

// ---------- misc ----------

#[tauri::command]
async fn get_changelog() -> Result<String, String> {
    // Latest release body from GitHub (fail-soft -> empty).
    let url = format!(
        "https://api.github.com/repos/{}/releases?per_page=5",
        fluxrec_core::constants::RELEASES_REPO
    );
    let text = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("FluxRecLauncher/1.0")
        .build()
        .map_err(|e| e.to_string())?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let releases: Vec<serde_json::Value> =
        serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for r in releases.iter().take(3) {
        let tag = r.get("tag_name").and_then(|v| v.as_str()).unwrap_or("?");
        let body = r.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let first: String = body.lines().take(4).collect::<Vec<_>>().join("\n");
        out.push_str(&format!("{tag}\n{first}\n\n"));
    }
    Ok(out)
}

#[tauri::command]
fn open_game_folder() -> Result<(), String> {
    let gd = game_dir();
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&gd)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(format!(
            "game folder: {} (open manually on this platform)",
            gd.display()
        ))
    }
}

#[tauri::command]
fn copy_diagnostics() -> Result<String, String> {
    // Redacted diagnostic bundle: versions + health, NO secrets.
    let settings = fluxrec_core::settings::LauncherSettings::load();
    let gd = settings.game_dir();
    let diag = serde_json::json!({
        "launcher": env!("CARGO_PKG_VERSION"),
        "channel": settings.channel,
        "game_dir": gd.to_string_lossy(),
        "game_present": fluxrec_core::launch::game_present(&gd),
        "bepinex": fluxrec_core::bepinex::status_detail(&gd),
        "plugin": fluxrec_core::plugin_sync::installed_hash(&gd).map(|h| h[..12.min(h.len())].to_string()),
        "content_version": fluxrec_core::update_content::local_version(&gd),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    });
    Ok(serde_json::to_string_pretty(&diag).map_err(|e| e.to_string())?)
}

#[tauri::command]
fn defender_steps() -> Vec<String> {
    fluxrec_core::defender::exclusion_steps(&game_dir())
}

fn main() {
    // `--apply-update <new-exe>`: Channel A helper mode (no UI).
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--apply-update" {
        let new_exe = PathBuf::from(&args[2]);
        let parent_pid = std::process::id();
        match fluxrec_core::update_launcher::apply_update_and_relaunch(&new_exe, parent_pid) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("apply-update failed: {e}");
                std::process::exit(1);
            }
        }
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_status,
            game_present,
            check_updates,
            apply_updates,
            fetch_launcher_update,
            play,
            verify_files,
            repair_files,
            repair_bepinex,
            reset_plugin,
            get_changelog,
            open_game_folder,
            copy_diagnostics,
            defender_steps,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
