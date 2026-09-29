//! BepInEx 6.0.0-pre.2 install / verify / repair.
//!
//! Install = download the upstream zip, extract into the game dir, verify the
//! preloader + core markers. Verify = check markers only (fast). Repair =
//! reinstall.

use crate::constants::*;
use crate::download::{self, ProgressCb};
use crate::{CoreError, Result};
use std::path::Path;

/// Markers that prove a working BepInEx install.
fn markers(game_dir: &Path) -> Vec<std::path::PathBuf> {
    vec![
        game_dir.join("BepInEx/core/BepInEx.Core.dll"),
        game_dir.join("BepInEx/core/BepInEx.Preloader.dll"),
        game_dir.join("doorstop_config.ini"),
        game_dir.join("winhttp.dll"),
    ]
}

/// Fast check: are all markers present?
pub fn is_installed(game_dir: &Path) -> bool {
    markers(game_dir).iter().all(|p| p.is_file())
}

/// Human-readable status for the health pill.
pub fn status_detail(game_dir: &Path) -> String {
    if is_installed(game_dir) {
        BEPINEX_VERSION.to_string()
    } else {
        let missing: Vec<_> = markers(game_dir)
            .iter()
            .filter(|p| !p.is_file())
            .map(|p| {
                p.strip_prefix(game_dir)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        format!("missing: {}", missing.join(", "))
    }
}

/// Install (or reinstall) BepInEx from the upstream release zip.
pub async fn install(game_dir: &Path, on_progress: Option<&ProgressCb>) -> Result<()> {
    let tmp_zip = std::env::temp_dir().join("bepinex-6.0.0-pre.2.zip");
    let staged = download::download_file(
        &[BEPINEX_URL.to_string()],
        &tmp_zip,
        None, // upstream provides no sha256 asset; markers verified after extract
        "install",
        &format!("BepInEx {BEPINEX_VERSION}"),
        on_progress,
    )
    .await?;
    let final_zip = tmp_zip.clone();
    download::commit_staged(&staged, &final_zip).await?;

    extract_zip(&final_zip, game_dir)?;
    let _ = std::fs::remove_file(&final_zip);

    if !is_installed(game_dir) {
        return Err(CoreError::Other(format!(
            "BepInEx install incomplete: {}",
            status_detail(game_dir)
        )));
    }
    Ok(())
}

/// Ensure BepInEx is present; install if not.
pub async fn ensure(game_dir: &Path, on_progress: Option<&ProgressCb>) -> Result<bool> {
    if is_installed(game_dir) {
        return Ok(false);
    }
    install(game_dir, on_progress).await?;
    Ok(true)
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| CoreError::Other(e.to_string()))?;
    std::fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| CoreError::Other(e.to_string()))?;
        let out = dest.join(entry.name());
        // Zip-slip guard.
        if !out.starts_with(dest) {
            return Err(CoreError::Other(format!("unsafe zip entry: {}", entry.name())));
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut outf = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut outf)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_install_detected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_installed(dir.path()));
        assert!(status_detail(dir.path()).contains("missing"));
    }

    #[test]
    fn full_marker_set_detected() {
        let dir = tempfile::tempdir().unwrap();
        for m in markers(dir.path()) {
            std::fs::create_dir_all(m.parent().unwrap()).unwrap();
            std::fs::write(&m, b"x").unwrap();
        }
        assert!(is_installed(dir.path()));
        assert_eq!(status_detail(dir.path()), BEPINEX_VERSION);
    }
}
