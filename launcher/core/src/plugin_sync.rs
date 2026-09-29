//! Channel C — plugin auto-update (sidecar hash pattern, unchanged design).
//!
//! Per launch: fetch `RecNetPlugin.dll.sha256` from the HF mirror, compare
//! with the installed DLL's hash, download on mismatch, verify, swap.
//! Fail-soft: offline or any error -> keep the installed DLL.
//! Anti-wipe rule: only `BepInEx/plugins/RecNetPlugin.dll` may be written.

use crate::constants::*;
use crate::download::{self, ProgressCb};
use crate::{CoreError, Result, UpdateCheck};
use std::path::Path;

fn dll_url() -> String {
    format!("{HF_MIRROR}/resolve/main/RecNetPlugin.dll")
}
fn sidecar_url() -> String {
    format!("{dll_url}.sha256", dll_url = dll_url())
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("FluxRecLauncher/1.0")
        .build()
        .expect("client")
}

/// Installed plugin hash, or None if missing.
pub fn installed_hash(game_dir: &Path) -> Option<String> {
    let p = game_dir.join(PLUGIN_REL_PATH);
    if p.is_file() {
        crate::sha256_file(&p).ok()
    } else {
        None
    }
}

/// Check the mirror sidecar against the installed DLL.
pub async fn check(game_dir: &Path) -> Result<UpdateCheck> {
    let current = installed_hash(game_dir)
        .map(|h| h[..12].to_string())
        .unwrap_or_else(|| "missing".into());
    let latest_full = http()
        .get(sidecar_url())
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let latest_full = latest_full.split_whitespace().next().unwrap_or("").to_string();
    let latest = if latest_full.len() >= 12 {
        latest_full[..12].to_string()
    } else {
        latest_full.clone()
    };
    let available = installed_hash(game_dir)
        .map(|h| !h.eq_ignore_ascii_case(&latest_full))
        .unwrap_or(true);
    Ok(UpdateCheck {
        channel: "plugin".into(),
        available,
        current,
        latest,
        detail: "RecNetPlugin.dll".into(),
    })
}

/// Sync the plugin. Fail-soft: returns Ok(false) when offline/missing so the
/// caller can keep the installed DLL and continue to Play.
pub async fn sync(game_dir: &Path, on_progress: Option<&ProgressCb>) -> Result<bool> {
    let sidecar = match http().get(sidecar_url()).send().await {
        Ok(r) => r,
        Err(_) => return Ok(false), // offline -> keep installed
    };
    if !sidecar.status().is_success() {
        return Ok(false);
    }
    let expected = sidecar
        .text()
        .await
        .map_err(|_| CoreError::Other("sidecar read failed".into()))?;
    let expected = expected.split_whitespace().next().unwrap_or("").to_string();
    if expected.is_empty() {
        return Ok(false);
    }

    if let Some(h) = installed_hash(game_dir) {
        if h.eq_ignore_ascii_case(&expected) {
            return Ok(true); // already current
        }
    }

    let dest = game_dir.join(PLUGIN_REL_PATH);
    // Remove the fossil stub from the old launcher line if present.
    let fossil = game_dir.join("BepInEx/plugins/FluxRec.Plugin.dll");
    let _ = std::fs::remove_file(&fossil);

    match download::download_file(
        &[dll_url()],
        &dest,
        Some(&expected),
        "plugin",
        "RecNetPlugin.dll",
        on_progress,
    )
    .await
    {
        Ok(staged) => {
            download::commit_staged(&staged, &dest).await?;
            Ok(true)
        }
        Err(_) => Ok(false), // fail-soft
    }
}
