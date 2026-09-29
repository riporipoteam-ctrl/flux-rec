//! Persisted launcher settings: `%LOCALAPPDATA%/FluxRec/launcher.json`.
//! Corrupt file -> defaults (never crash on bad settings).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherSettings {
    /// "stable" or "beta"
    pub channel: String,
    /// game install dir override (empty = default)
    pub install_dir: String,
    /// check for updates on launch
    pub check_updates_on_launch: bool,
    /// close launcher when the game starts
    pub close_on_play: bool,
    /// hide the game console window
    pub no_console: bool,
    /// include hardware info in diagnostics
    pub hw_in_diagnostics: bool,
    /// first-run wizard completed
    pub firstrun_done: bool,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            channel: "stable".into(),
            install_dir: String::new(),
            check_updates_on_launch: true,
            close_on_play: true,
            no_console: true,
            hw_in_diagnostics: false,
            firstrun_done: false,
        }
    }
}

impl LauncherSettings {
    fn path() -> PathBuf {
        crate::launcher_data_dir().join("launcher.json")
    }

    /// Load settings; fall back to defaults on any error. Validates values.
    pub fn load() -> Self {
        let p = Self::path();
        let mut s: Self = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.validate();
        s
    }

    pub fn save(&self) -> crate::Result<()> {
        let p = Self::path();
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }

    fn validate(&mut self) {
        if self.channel != "beta" {
            self.channel = "stable".into();
        }
        if !self.install_dir.is_empty() {
            let pb = PathBuf::from(&self.install_dir);
            if pb.to_string_lossy().is_empty() {
                self.install_dir.clear();
            }
        }
    }

    /// Effective game install dir.
    pub fn game_dir(&self) -> PathBuf {
        if self.install_dir.is_empty() {
            crate::default_install_dir()
        } else {
            PathBuf::from(&self.install_dir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_settings_fall_back_to_defaults() {
        let s: LauncherSettings = serde_json::from_str("{not json").unwrap_or_default();
        assert_eq!(s.channel, "stable");
        assert!(s.check_updates_on_launch);
    }

    #[test]
    fn bad_channel_is_normalized() {
        let mut s = LauncherSettings {
            channel: "nightly".into(),
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.channel, "stable");
    }
}
