//! Game launch: self-heal order, then spawn.
//!
//! Order (matches the old launcher's proven flow):
//! 1. BepInEx ensure (install if markers missing)
//! 2. Plugin sync (fail-soft)
//! 3. Goldberg bypass check (Windows)
//! 4. Spawn RecRoom.exe
//!
//! Every step fails soft toward "play anyway" — the launcher never strands
//! the player.

use crate::constants::*;
use crate::download::ProgressCb;
use crate::{CoreError, HealthStatus, Result};
use std::path::Path;

/// Compute the home-screen health pills.
pub async fn health(game_dir: &Path) -> HealthStatus {
    let game_files_ok = game_dir.join("RecRoom.exe").is_file();
    let game_detail = if game_files_ok {
        format!("build {GAME_BUILD}")
    } else {
        "RecRoom.exe missing".into()
    };

    let bepinex_ok = crate::bepinex::is_installed(game_dir);
    let bepinex_detail = crate::bepinex::status_detail(game_dir);

    let plugin_hash = crate::plugin_sync::installed_hash(game_dir);
    let plugin_ok = plugin_hash.is_some();
    let plugin_detail = plugin_hash
        .map(|h| format!("{}…", &h[..12.min(h.len())]))
        .unwrap_or_else(|| "not installed".into());

    // Backend ping: quick HEAD against the econ worker (fail-soft).
    let (backend_ok, backend_detail) = backend_ping().await;

    HealthStatus {
        game_files_ok,
        game_detail,
        bepinex_ok,
        bepinex_detail,
        plugin_ok,
        plugin_detail,
        backend_ok,
        backend_detail,
    }
}

async fn backend_ping() -> (bool, String) {
    let url = "https://fluxrec-econ.ripo-ripoteam.workers.dev/health";
    let start = std::time::Instant::now();
    let ok = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("client")
        .get(url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    let ms = start.elapsed().as_millis();
    if ok {
        (true, format!("{ms} ms"))
    } else {
        (false, "unreachable".into())
    }
}

/// Run the self-heal order. Returns a list of human-readable actions taken.
pub async fn self_heal(game_dir: &Path, on_progress: Option<&ProgressCb>) -> Result<Vec<String>> {
    let mut actions = Vec::new();
    if crate::bepinex::ensure(game_dir, on_progress).await? {
        actions.push(format!("installed BepInEx {BEPINEX_VERSION}"));
    }
    match crate::plugin_sync::sync(game_dir, on_progress).await {
        Ok(true) => actions.push("plugin up to date".into()),
        Ok(false) => actions.push("plugin sync skipped (offline?)".into()),
        Err(e) => actions.push(format!("plugin sync failed (continuing): {e}")),
    }
    #[cfg(windows)]
    {
        match crate::bypass::ensure_goldberg(game_dir, HF_MIRROR) {
            Ok(true) => actions.push("steam bypass installed".into()),
            Ok(false) => {}
            Err(e) => actions.push(format!("bypass check failed (continuing): {e}")),
        }
        let missing = crate::bypass::missing_hosts_entries();
        if !missing.is_empty() {
            actions.push(format!(
                "hosts entries missing (needs elevation): {}",
                missing.join(", ")
            ));
        }
    }
    Ok(actions)
}

/// Spawn the game. Windows-only in production.
#[cfg(windows)]
pub fn spawn_game(game_dir: &Path, no_console: bool) -> Result<u32> {
    use std::os::windows::process::CommandExt;
    let exe = game_dir.join("RecRoom.exe");
    if !exe.is_file() {
        return Err(CoreError::Other("RecRoom.exe not found — install the game first".into()));
    }
    let mut cmd = std::process::Command::new(&exe);
    // The plugin redirects API traffic; no CLI flags needed for the bypass.
    if no_console {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let child = cmd.current_dir(game_dir).spawn()?;
    Ok(child.id())
}

#[cfg(not(windows))]
pub fn spawn_game(_game_dir: &Path, _no_console: bool) -> Result<u32> {
    Err(CoreError::UnsupportedPlatform(
        "game launch is Windows-only".into(),
    ))
}

/// Find the game exe (also used by the UI to decide firstrun vs home).
pub fn game_present(game_dir: &Path) -> bool {
    game_dir.join("RecRoom.exe").is_file()
}
