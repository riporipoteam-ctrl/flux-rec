//! Flux Rec launcher core — platform-neutral business logic.
//!
//! Three independent update channels (the anti-wipe rule):
//! - Channel A (`update_launcher`): the launcher binary itself, via GitHub releases.
//! - Channel B (`update_content`): game files, via manifest.json on the HF mirror.
//! - Channel C (`plugin_sync`): RecNetPlugin.dll, via .sha256 sidecar on the HF mirror.
//!
//! A channel update may only write inside its own file set. Never cross channels.

pub mod bepinex;
pub mod bypass;
pub mod defender;
pub mod download;
pub mod launch;
pub mod plugin_sync;
pub mod settings;
pub mod update_content;
pub mod update_launcher;

use serde::{Deserialize, Serialize};

/// Well-known constants. Change mirror/repo here, not scattered through code.
pub mod constants {
    /// Hugging Face dataset mirror for game content + plugin.
    pub const HF_MIRROR: &str = "https://huggingface.co/datasets/Echoxr/rrflux-game";
    /// GitHub repo that publishes launcher releases.
    pub const RELEASES_REPO: &str = "riporipoteam-ctrl/flux-rec";
    /// Release tag prefix for launcher builds (kept from the old line).
    pub const LAUNCHER_TAG_PREFIX: &str = "recflare-installer-v";
    /// BepInEx version the game needs.
    pub const BEPINEX_VERSION: &str = "6.0.0-pre.2";
    /// BepInEx download URL (Unity IL2CPP, Win x64).
    pub const BEPINEX_URL: &str = "https://github.com/BepInEx/BepInEx/releases/download/v6.0.0-pre.2/BepInEx-Unity.IL2CPP-win-x64-6.0.0-pre.2.zip";
    /// Game client build identifier.
    pub const GAME_BUILD: &str = "20230414";
    /// Name of the version-token file inside the game dir.
    pub const CONTENT_VERSION_FILE: &str = ".fluxrec_content_version";
    /// Plugin file name (relative to game dir).
    pub const PLUGIN_REL_PATH: &str = "BepInEx/plugins/RecNetPlugin.dll";
}

/// Progress event emitted during long operations. Forwarded to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    /// "launcher" | "content" | "plugin" | "verify" | "install"
    pub channel: String,
    pub message: String,
    /// bytes done / total for the current file (0/0 when indeterminate)
    pub file_done: u64,
    pub file_total: u64,
    /// overall 0.0..=1.0
    pub overall: f32,
}

/// Health pill state for the home screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub game_files_ok: bool,
    pub game_detail: String,
    pub bepinex_ok: bool,
    pub bepinex_detail: String,
    pub plugin_ok: bool,
    pub plugin_detail: String,
    pub backend_ok: bool,
    pub backend_detail: String,
}

/// Per-channel update availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub channel: String,
    pub available: bool,
    pub current: String,
    pub latest: String,
    pub detail: String,
}

/// Unified error type.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("hash mismatch for {0}: expected {1}, got {2}")]
    HashMismatch(String, String, String),
    #[error("not supported on this platform: {0}")]
    UnsupportedPlatform(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

/// SHA-256 hex of a file.
pub fn sha256_file(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

/// SHA-256 hex of bytes.
pub fn sha256_bytes(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}

/// Resolve the game install dir: settings override, else default.
pub fn default_install_dir() -> std::path::PathBuf {
    if cfg!(windows) {
        dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("C:\\"))
            .join("FluxRec")
            .join("game")
    } else {
        // Linux dev/test default (never used in production).
        std::path::PathBuf::from("/tmp/fluxrec-game")
    }
}

/// Launcher data dir (settings, caches).
pub fn launcher_data_dir() -> std::path::PathBuf {
    if cfg!(windows) {
        dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("C:\\"))
            .join("FluxRec")
    } else {
        std::path::PathBuf::from("/tmp/fluxrec-launcher")
    }
}
