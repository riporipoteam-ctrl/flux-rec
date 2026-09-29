//! Channel B — differential game content update.
//!
//! Source: `manifest.json` on the HF mirror:
//! ```json
//! { "content_version": 14,
//!   "files": [ {"path":"RecRoom_Data/...","sha256":"…","size":123,
//!                "url":"https://…"} ] }
//! ```
//! Anti-wipe rule: this channel may ONLY write files listed in the manifest.
//! It never touches the launcher exe. The version token is written LAST,
//! only after every file verifies.

use crate::constants::*;
use crate::download::{self, ProgressCb};
use crate::{CoreError, ProgressEvent, Result, UpdateCheck};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContentManifest {
    pub content_version: u64,
    pub files: Vec<ManifestFile>,
}

/// Local content version (0 = nothing installed).
pub fn local_version(game_dir: &Path) -> u64 {
    std::fs::read_to_string(game_dir.join(CONTENT_VERSION_FILE))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn manifest_url() -> String {
    format!("{HF_MIRROR}/resolve/main/manifest.json")
}

/// Fetch the remote manifest.
pub async fn fetch_manifest() -> Result<ContentManifest> {
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("FluxRecLauncher/1.0")
        .build()
        .expect("client")
        .get(manifest_url())
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(CoreError::Other(format!(
            "manifest fetch returned {}",
            resp.status()
        )));
    }
    Ok(resp.json().await?)
}

/// Check whether a content update is available.
pub async fn check(game_dir: &Path) -> Result<UpdateCheck> {
    let current = local_version(game_dir).to_string();
    let manifest = fetch_manifest().await?;
    let latest = manifest.content_version.to_string();
    let available = manifest.content_version > local_version(game_dir);
    Ok(UpdateCheck {
        channel: "content".into(),
        current,
        latest,
        available,
        detail: format!("{} files", manifest.files.len()),
    })
}

/// Result of a sync run.
#[derive(Debug, Default)]
pub struct SyncReport {
    pub downloaded: u64,
    pub verified_ok: u64,
    pub failed: Vec<String>,
}

/// Differential sync: download only missing/changed files, verify each,
/// commit atomically, write the version token last.
pub async fn sync(
    game_dir: &Path,
    manifest: &ContentManifest,
    on_progress: Option<&ProgressCb>,
) -> Result<SyncReport> {
    // Safety: refuse to write outside the game dir (path traversal in manifest).
    for f in &manifest.files {
        let p = Path::new(&f.path);
        if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(CoreError::Other(format!(
                "manifest contains unsafe path: {}",
                f.path
            )));
        }
    }

    // 1. Walk: queue files whose local hash differs.
    let mut queue: Vec<&ManifestFile> = Vec::new();
    let mut verified_ok = 0u64;
    for f in &manifest.files {
        let local = game_dir.join(&f.path);
        let ok = local.is_file()
            && std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0) == f.size
            && crate::sha256_file(&local)
                .map(|h| h.eq_ignore_ascii_case(&f.sha256))
                .unwrap_or(false);
        if ok {
            verified_ok += 1;
        } else {
            queue.push(f);
        }
    }

    // 2. Download + verify + commit each queued file.
    let total = queue.len() as u64;
    let mut downloaded = 0u64;
    let mut failed = Vec::new();
    for (i, f) in queue.iter().enumerate() {
        let dest = game_dir.join(&f.path);
        // Per-file progress is forwarded as-is; overall progress is emitted
        // separately (avoids 'static closure lifetime issues).
        match download::download_file(
            &[f.url.clone()],
            &dest,
            Some(&f.sha256),
            "content",
            &f.path,
            on_progress,
        )
        .await
        {
            Ok(staged) => {
                if let Err(e) = download::commit_staged(&staged, &dest).await {
                    failed.push(format!("{}: {e}", f.path));
                } else {
                    downloaded += 1;
                }
            }
            Err(e) => failed.push(format!("{}: {e}", f.path)),
        }
        if let Some(cb) = on_progress {
            cb(ProgressEvent {
                channel: "content".into(),
                message: format!("{} ({}/{})", f.path, i + 1, total),
                file_done: (i + 1) as u64,
                file_total: total,
                overall: if total > 0 {
                    (i + 1) as f32 / total as f32
                } else {
                    1.0
                },
            });
        }
    }

    // 3. Version token LAST — only when everything succeeded.
    if failed.is_empty() {
        let tmp = game_dir.join(format!("{CONTENT_VERSION_FILE}.tmp"));
        std::fs::write(&tmp, manifest.content_version.to_string())?;
        std::fs::rename(&tmp, game_dir.join(CONTENT_VERSION_FILE))?;
    }

    Ok(SyncReport {
        downloaded,
        verified_ok,
        failed,
    })
}

/// Verify-only pass (Verify/Repair screen): returns mismatched paths.
pub fn verify_against_manifest(game_dir: &Path, manifest: &ContentManifest) -> Vec<String> {
    manifest
        .files
        .iter()
        .filter(|f| {
            let local = game_dir.join(&f.path);
            !(local.is_file()
                && crate::sha256_file(&local)
                    .map(|h| h.eq_ignore_ascii_case(&f.sha256))
                    .unwrap_or(false))
        })
        .map(|f| f.path.clone())
        .collect()
}

#[allow(dead_code)]
fn _local_version_pathbuf(game_dir: &PathBuf) -> u64 {
    local_version(game_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_manifest_paths_rejected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let m = ContentManifest {
            content_version: 1,
            files: vec![ManifestFile {
                path: "../evil.dll".into(),
                sha256: "x".into(),
                size: 1,
                url: "https://example.com/x".into(),
            }],
        };
        let dir = std::env::temp_dir().join("fluxrec-test-traversal");
        let err = rt
            .block_on(sync(&dir, &m, None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("unsafe path"));
    }

    #[test]
    fn version_token_parses() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(local_version(dir.path()), 0);
        std::fs::write(dir.path().join(CONTENT_VERSION_FILE), "14\n").unwrap();
        assert_eq!(local_version(dir.path()), 14);
    }

    #[test]
    fn verify_detects_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        let m = ContentManifest {
            content_version: 1,
            files: vec![ManifestFile {
                path: "a.txt".into(),
                sha256: crate::sha256_bytes(b"different"),
                size: 5,
                url: "https://example.com/a".into(),
            }],
        };
        let bad = verify_against_manifest(dir.path(), &m);
        assert_eq!(bad, vec!["a.txt".to_string()]);
    }

    #[test]
    fn verify_passes_on_match() {
        let dir = tempfile::tempdir().unwrap();
        let data = b"hello";
        std::fs::write(dir.path().join("a.txt"), data).unwrap();
        let m = ContentManifest {
            content_version: 1,
            files: vec![ManifestFile {
                path: "a.txt".into(),
                sha256: crate::sha256_bytes(data),
                size: 5,
                url: "https://example.com/a".into(),
            }],
        };
        assert!(verify_against_manifest(dir.path(), &m).is_empty());
    }
}
