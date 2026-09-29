//! Windows Defender handling — guided manual exclusion only.
//!
//! Standing rule: the Defender COM-API approach is blocked (safety filters).
//! The launcher checks whether the game dir looks excluded/quarantined and,
//! if not, shows the guided manual-exclusion steps in the UI. It never
//! touches Defender programmatically.

use crate::Result;
use std::path::Path;

/// Heuristic: did Defender (or another AV) likely quarantine files?
/// We check for the canary: the plugin DLL and preloader existing.
pub fn quarantine_suspected(game_dir: &Path) -> bool {
    let plugin = game_dir.join(crate::constants::PLUGIN_REL_PATH);
    let preloader = game_dir.join("BepInEx/core/BepInEx.Preloader.dll");
    // If BepInEx was installed before (marker dir exists) but the DLLs are
    // gone, something removed them — most likely AV.
    let bepinex_dir = game_dir.join("BepInEx");
    bepinex_dir.is_dir() && (!plugin.is_file() || !preloader.is_file())
}

/// Guided steps shown in the UI (Windows only; on other platforms this is informational).
pub fn exclusion_steps(game_dir: &Path) -> Vec<String> {
    vec![
        "Open Windows Security > Virus & threat protection > Manage settings.".into(),
        "Under Exclusions, choose Add an exclusion > Folder.".into(),
        format!("Add this folder: {}", game_dir.display()),
        "Re-run Verify in the launcher to restore any quarantined files.".into(),
    ]
}

/// Placeholder for a future status check; today we only report the heuristic.
pub fn status(game_dir: &Path) -> String {
    if quarantine_suspected(game_dir) {
        "files missing — AV quarantine suspected, see Verify".into()
    } else {
        "no quarantine detected".into()
    }
}

#[allow(dead_code)]
fn _ok(_p: &Path) -> Result<()> {
    Ok(())
}
