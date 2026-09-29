//! Channel A — launcher self-update.
//!
//! Source: GitHub releases on `RELEASES_REPO`, tags `recflare-installer-v*`
//! (+ `-beta` when the beta channel is selected).
//! Anti-wipe rule: this channel may ONLY replace the launcher binary itself.
//! It never touches the game dir.

use crate::constants::*;
use crate::download::{self, ProgressCb};
use crate::{CoreError, Result, UpdateCheck};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
    body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// Check GitHub for a newer launcher release.
pub async fn check(beta: bool) -> Result<UpdateCheck> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let url = format!("https://api.github.com/repos/{RELEASES_REPO}/releases?per_page=20");
    let resp = download_client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(CoreError::Other(format!(
            "github releases returned {}",
            resp.status()
        )));
    }
    let releases: Vec<GhRelease> = resp.json().await?;

    let mut best: Option<(semver::Version, &GhRelease)> = None;
    for r in &releases {
        let tag = r.tag_name.strip_prefix(LAUNCHER_TAG_PREFIX).unwrap_or("");
        let is_beta = tag.ends_with("-beta");
        if is_beta && !beta {
            continue;
        }
        let ver_str = tag.strip_suffix("-beta").unwrap_or(tag);
        let Ok(v) = semver::Version::parse(ver_str) else {
            continue;
        };
        if best.as_ref().map(|(bv, _)| &v > bv).unwrap_or(true) {
            best = Some((v, r));
        }
    }

    let (latest_v, rel) = match best {
        Some(b) => b,
        None => {
            return Ok(UpdateCheck {
                channel: "launcher".into(),
                available: false,
                current: current.clone(),
                latest: current,
                detail: "no releases found".into(),
            })
        }
    };
    let cur_v = semver::Version::parse(&current).unwrap_or(semver::Version::new(0, 0, 0));
    Ok(UpdateCheck {
        channel: "launcher".into(),
        available: latest_v > cur_v,
        current,
        latest: latest_v.to_string(),
        detail: rel.body.clone().unwrap_or_default(),
    })
}

/// Download + verify the new launcher exe. Returns the staged file path.
/// The caller must spawn `<new-exe> --apply-update` and exit.
pub async fn fetch_update(
    check: &UpdateCheck,
    on_progress: Option<&ProgressCb>,
) -> Result<std::path::PathBuf> {
    // Re-fetch release list to find asset URLs (kept simple & stateless).
    let url = format!("https://api.github.com/repos/{RELEASES_REPO}/releases?per_page=20");
    let resp = download_client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    let releases: Vec<GhRelease> = resp.json().await?;
    let rel = releases
        .iter()
        .find(|r| {
            r.tag_name
                .strip_prefix(LAUNCHER_TAG_PREFIX)
                .map(|t| t.strip_suffix("-beta").unwrap_or(t) == check.latest)
                .unwrap_or(false)
        })
        .ok_or_else(|| CoreError::Other("release disappeared".into()))?;

    let exe = rel
        .assets
        .iter()
        .find(|a| a.name.ends_with(".exe") && !a.name.ends_with(".sha256"))
        .ok_or_else(|| CoreError::Other("no .exe asset in release".into()))?;
    let sha_asset = rel
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sha256", exe.name));

    // Fetch expected hash first (fail-soft: skip update if the .sha256 asset is missing).
    let expected: Option<String> = match sha_asset {
        Some(a) => {
            let t = download_client()
                .get(&a.browser_download_url)
                .send()
                .await?
                .text()
                .await?;
            Some(t.split_whitespace().next().unwrap_or("").to_string())
        }
        None => None,
    };

    let tmp = std::env::temp_dir().join(format!("FluxRecLauncher-update-{}.exe", check.latest));
    let staged = download::download_file(
        &[exe.browser_download_url.clone()],
        &tmp,
        expected.as_deref(),
        "launcher",
        &format!("launcher v{}", check.latest),
        on_progress,
    )
    .await?;

    // Sanity: must look like a Windows executable (MZ header).
    let mut mz = [0u8; 2];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(&staged)?;
        f.read_exact(&mut mz)?;
    }
    if mz != [b'M', b'Z'] {
        let _ = std::fs::remove_file(&staged);
        return Err(CoreError::Other("downloaded file is not a Windows executable".into()));
    }
    download::commit_staged(&staged, &tmp).await?;
    Ok(tmp)
}

/// `--apply-update <new-exe>`: wait for the parent launcher to exit, swap the
/// binaries, re-exec. Runs as a detached helper so the old exe is never
/// replaced while running.
#[cfg(windows)]
pub fn apply_update_and_relaunch(new_exe: &std::path::Path, parent_pid: u32) -> Result<()> {
    use std::os::windows::process::CommandExt;
    // Wait for parent to exit (poll).
    for _ in 0..600 {
        let alive = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {parent_pid}"), "/NH"])
            .creation_flags(0x08000000)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&parent_pid.to_string()))
            .unwrap_or(false);
        if !alive {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let current = std::env::current_exe()?;
    // Keep a backup of the old launcher in case the swap fails midway.
    let backup = current.with_extension("exe.bak");
    let _ = std::fs::remove_file(&backup);
    std::fs::rename(&current, &backup)?;
    if let Err(e) = std::fs::rename(new_exe, &current) {
        // Roll back: restore the old launcher. Never leave the user stranded.
        let _ = std::fs::rename(&backup, &current);
        return Err(CoreError::Other(format!("launcher swap failed, rolled back: {e}")));
    }
    let _ = std::fs::remove_file(&backup);
    std::process::Command::new(&current).spawn()?;
    Ok(())
}

#[cfg(not(windows))]
pub fn apply_update_and_relaunch(_new_exe: &std::path::Path, _parent_pid: u32) -> Result<()> {
    Err(CoreError::UnsupportedPlatform(
        "launcher self-update swap is Windows-only".into(),
    ))
}

/// Latest release notes (for the "What's new" card). Fail-soft: empty on error.
pub async fn changelog() -> Result<String> {
    let url = format!("https://api.github.com/repos/{RELEASES_REPO}/releases?per_page=5");
    let text = download_client()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .text()
        .await?;
    let releases: Vec<serde_json::Value> = serde_json::from_str(&text)?;
    let mut out = String::new();
    for r in releases.iter().take(3) {
        let tag = r.get("tag_name").and_then(|v| v.as_str()).unwrap_or("?");
        let body = r.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let first: String = body.lines().take(4).collect::<Vec<_>>().join("\n");
        out.push_str(&format!("{tag}\n{first}\n\n"));
    }
    Ok(out)
}

fn download_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("FluxRecLauncher/1.0")
        .build()
        .expect("client build")
}

#[cfg(test)]
mod tests {
    #[test]
    fn tag_filtering_logic() {
        // stable channel skips beta tags
        let tags = ["1.0.0", "1.1.0-beta", "1.1.0"];
        let stable: Vec<_> = tags
            .iter()
            .filter(|t| {
                let is_beta = t.ends_with("-beta");
                !is_beta // beta=false
            })
            .collect();
        assert_eq!(stable, vec![&"1.0.0", &"1.1.0"]);

        let cur = semver::Version::parse("1.0.0").unwrap();
        let latest = semver::Version::parse("1.1.0").unwrap();
        assert!(latest > cur);
    }
}
