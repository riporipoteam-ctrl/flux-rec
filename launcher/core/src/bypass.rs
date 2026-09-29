//! Steam bypass: Goldberg emulator files + hosts entries.
//!
//! The game must never require Steam. On Windows the launcher ensures:
//! - `steam_api64.dll` (Goldberg) next to the game exe
//! - `steam_emu.ini` with the offline config
//! Hosts entries are verified read-only here; editing them needs elevation
//! and is done by the Tauri frontend flow (explicit user consent screen).

use crate::Result;
use std::path::Path;

/// Goldberg files live next to the game executable.
pub fn ensure_goldberg(game_dir: &Path, mirror_base: &str) -> Result<bool> {
    #[cfg(not(windows))]
    {
        let _ = (game_dir, mirror_base);
        return Ok(false); // nothing to do on non-Windows
    }
    #[cfg(windows)]
    {
        let dll = game_dir.join("steam_api64.dll");
        let ini = game_dir.join("steam_emu.ini");
        if dll.is_file() && ini.is_file() {
            return Ok(false);
        }
        // Fetch from the mirror (blocking runtime for simplicity in this helper).
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| crate::CoreError::Other(e.to_string()))?;
        rt.block_on(async {
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("client");
            for (name, dest) in [("steam_api64.dll", &dll), ("steam_emu.ini", &ini)] {
                let url = format!("{mirror_base}/resolve/main/goldberg/{name}");
                let bytes = http
                    .get(&url)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?;
                std::fs::write(dest, &bytes)?;
            }
            Ok::<_, crate::CoreError>(true)
        })
    }
}

/// Read-only check: are the required hosts entries present?
/// Returns the list of missing entries (empty = all good).
#[cfg(windows)]
pub fn missing_hosts_entries() -> Vec<String> {
    const NEEDED: &[&str] = &["127.0.0.1 api.rec.net"];
    let Ok(content) = std::fs::read_to_string(r"C:\Windows\System32\drivers\etc\hosts") else {
        return NEEDED.iter().map(|s| s.to_string()).collect();
    };
    NEEDED
        .iter()
        .filter(|n| !content.lines().any(|l| l.trim() == **n))
        .map(|s| s.to_string())
        .collect()
}

#[cfg(not(windows))]
pub fn missing_hosts_entries() -> Vec<String> {
    Vec::new()
}
