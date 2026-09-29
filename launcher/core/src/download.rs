//! Resumable file downloader with mirror fallback.
//!
//! - Tries mirrors in order; first success wins.
//! - Resume via HTTP Range when the server supports it and a `.part` file exists.
//! - Verifies SHA-256 (when expected is provided) BEFORE the file is moved into place.
//! - Atomic: downloads to `<dest>.part`, renames to `<dest>.new`, caller renames.

use crate::{CoreError, ProgressEvent, Result};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub type ProgressCb = Box<dyn Fn(ProgressEvent) + Send + Sync>;

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("FluxRecLauncher/1.0")
        .build()
        .expect("client build")
}

/// Download `urls[0..]` (fallback order) to `dest`.
/// If `expected_sha256` is Some, the hash is verified before rename.
pub async fn download_file(
    urls: &[String],
    dest: &Path,
    expected_sha256: Option<&str>,
    channel: &str,
    label: &str,
    on_progress: Option<&ProgressCb>,
) -> Result<PathBuf> {
    let mut last_err = CoreError::Other("no mirrors provided".into());
    for url in urls {
        match download_one(url, dest, expected_sha256, channel, label, on_progress).await {
            Ok(p) => return Ok(p),
            Err(e) => {
                last_err = e;
                continue;
            }
        }
    }
    Err(last_err)
}

async fn download_one(
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    channel: &str,
    label: &str,
    on_progress: Option<&ProgressCb>,
) -> Result<PathBuf> {
    let part = dest.with_extension("part");
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Resume offset: existing .part size.
    let resume_from: u64 = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    let mut req = client().get(url);
    if resume_from > 0 {
        req = req.header("Range", format!("bytes={resume_from}-"));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() && resp.status().as_u16() != 206 {
        return Err(CoreError::Other(format!(
            "mirror {} returned {}",
            url,
            resp.status()
        )));
    }
    let resumed = resp.status().as_u16() == 206;
    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|len| if resumed { len + resume_from } else { len })
        .unwrap_or(0);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(!resumed)
        .open(&part)
        .await?;
    if resumed {
        use tokio::io::AsyncSeekExt;
        file.seek(std::io::SeekFrom::End(0)).await?;
    }

    let mut stream = resp.bytes_stream();
    let mut done = resume_from;
    let emit = |done: u64| {
        if let Some(cb) = on_progress {
            cb(ProgressEvent {
                channel: channel.into(),
                message: format!("{label}"),
                file_done: done,
                file_total: total,
                overall: if total > 0 {
                    (done as f32 / total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            });
        }
    };
    let mut tick = 0u32;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(CoreError::Http)?;
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        tick += 1;
        if tick % 16 == 0 {
            emit(done);
        }
    }
    file.flush().await?;
    drop(file);
    emit(done);

    // Verify hash before the file goes anywhere near its final name.
    if let Some(expected) = expected_sha256 {
        let actual = crate::sha256_file(&part)?;
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(CoreError::HashMismatch(
                label.into(),
                expected.into(),
                actual,
            ));
        }
    }

    // Atomic-ish: .part -> .new ; caller does the final rename.
    let staged = dest.with_extension("new");
    tokio::fs::rename(&part, &staged).await?;
    Ok(staged)
}

/// Move a staged `.new` file into its final place (atomic rename).
pub async fn commit_staged(staged: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::rename(staged, dest).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn progress_event_serializes() {
        let e = crate::ProgressEvent {
            channel: "content".into(),
            message: "x".into(),
            file_done: 1,
            file_total: 2,
            overall: 0.5,
        };
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("content"));
    }
}
